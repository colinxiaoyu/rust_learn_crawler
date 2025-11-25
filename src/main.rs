use std::sync::Arc;

use anyhow::Result;
use futures::{StreamExt, stream::FuturesUnordered};
use reqwest::Url;
use scraper::{Html, Selector};
use tokio::sync::Semaphore;

#[tokio::main]
async fn main() -> Result<()> {
    let urls = vec![
        "https://www.rust-lang.org/",
        "https://www.mozilla.org/",
        "https://www.wikipedia.org/",
        "https://www.github.com/",
        "https://www.reddit.com/",
    ];

    let max_concurrent = 2;
    let semaphore = Arc::new(Semaphore::new(max_concurrent));

    let mut tasks = FuturesUnordered::new();

    for url in urls {
        let sem_clone = Arc::clone(&semaphore);
        let url_clone = url.to_string();

        tasks.push(tokio::spawn(async move {
            let _permit = sem_clone.acquire().await.unwrap();
            fetch_title(url_clone).await
        }));
    }

    while let Some(result) = tasks.next().await {
        match result {
            Ok(Ok((url, title, links))) => {
                println!("URL: {}", url);
                println!("TITLE: {}", title);
                println!("LINKS:");
                for l in links {
                    println!("  {}", l);
                }
                println!("---------------------------------------");
            }
            Ok(Err(e)) => eprintln!("Error: {}", e),
            Err(e) => eprintln!("Error: {}", e),
        }
    }

    Ok(())
}

async fn fetch_title(url: String) -> Result<(String, String, Vec<String>)> {
    let base_url = Url::parse(&url)?;

    let body = reqwest::get(&url).await?.text().await?;

    let document = Html::parse_document(&body);
    let selector = Selector::parse("title").unwrap();

    let title = document
        .select(&selector)
        .next()
        .map(|t| t.inner_html())
        .unwrap_or_else(|| "No title".to_string());

    let link_selector = Selector::parse("a").unwrap();
    let mut links = Vec::new();

    for element in document.select(&link_selector) {
        if let Some(href) = element.value().attr("href") {
            if let Ok(abs_url) = base_url.join(href) {
                links.push(abs_url.to_string());
            }
        }
    }

    Ok((url, title, links))
}
