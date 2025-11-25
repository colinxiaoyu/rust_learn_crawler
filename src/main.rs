use anyhow::Result;
use dashmap::DashSet;
use futures::future::join_all;
use reqwest::Client;
use scraper::{Html, Selector};
use std::sync::Arc;
use tokio::sync::{Semaphore, mpsc};
use tokio::time::{Duration, sleep};
use url::Url;

#[derive(Debug, Clone)]
struct CrawlTask {
    url: String,
    depth: usize,
}

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() -> Result<()> {
    let seeds = vec!["https://www.rust-lang.org/", "https://www.mozilla.org/"];

    let max_depth = 2;
    let max_concurrent_requests = 10;
    let worker_count = 4;
    let request_timeout = Duration::from_secs(15);

    let client = Client::builder()
        .user_agent("rust-crawler/0.1 (+https://example.com/bot)")
        .timeout(request_timeout)
        .build()?;

    let visited = Arc::new(DashSet::new());
    let semaphore = Arc::new(Semaphore::new(max_concurrent_requests));

    // Tokio mpsc channel
    let (tx, mut rx) = mpsc::channel::<CrawlTask>(100);

    // Seed initial tasks
    for s in seeds {
        tx.send(CrawlTask {
            url: s.to_string(),
            depth: 2,
        })
        .await?;
    }

    // Spawn worker tasks
    let mut handles = vec![];
    for i in 0..worker_count {
        let client = client.clone();
        let visited = visited.clone();
        let semaphore = semaphore.clone();
        let tx_clone = tx.clone();
        let mut rx = rx; // Receiver 移动到 worker

        let h = tokio::spawn(async move {
            while let Some(task) = rx.recv().await {
                if visited.contains(&task.url) {
                    continue;
                }
                visited.insert(task.url.clone());

                // Semaphore 限制并发
                let _permit = semaphore.acquire().await.unwrap();

                let res = fetch_page(&client, &task.url).await;
                match res {
                    Ok(body) => {
                        println!("[worker {}] fetched {} (depth {})", i, task.url, task.depth);

                        if let Ok((title, links)) = parse_title_and_links(&task.url, &body) {
                            println!("  TITLE: {}", title);

                            if task.depth < max_depth {
                                for link in links {
                                    if !link.starts_with("http") || visited.contains(&link) {
                                        continue;
                                    }
                                    tx_clone
                                        .send(CrawlTask {
                                            url: link,
                                            depth: task.depth + 1,
                                        })
                                        .await
                                        .unwrap();
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[worker {}] failed to fetch {}: {:?}", i, task.url, e);
                    }
                }
            }
            println!("[worker {}] exiting", i);
        });

        handles.push(h);
        // 这里只能移动 rx 一次，所以只给第一个 worker
        break;
    }

    join_all(handles).await;
    println!("Crawl finished. Visited count = {}", visited.len());

    Ok(())
}

async fn fetch_page(client: &Client, url: &str) -> Result<String> {
    let resp = client.get(url).send().await?.error_for_status()?;
    Ok(resp.text().await?)
}

fn parse_title_and_links(base: &str, body: &str) -> Result<(String, Vec<String>)> {
    let base_url = Url::parse(base)?;
    let document = Html::parse_document(body);

    let title_selector = Selector::parse("title").unwrap();
    let title = document
        .select(&title_selector)
        .next()
        .map(|t| t.inner_html())
        .unwrap_or_default();

    let mut links = Vec::new();
    let link_selector = Selector::parse("a").unwrap();
    for el in document.select(&link_selector) {
        if let Some(href) = el.value().attr("href") {
            if let Ok(abs) = base_url.join(href) {
                links.push(abs.into_string());
            }
        }
    }

    Ok((title, links))
}
