use headless_chrome::{Browser, LaunchOptionsBuilder};
use regex::Regex;
use reqwest::Client;
use scraper::{Html, Selector};
use std::collections::HashSet;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use tokio::fs::File as AsyncFile;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::time::Duration;
use std::process::Command;
#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <domain> ", args[0]);
        return;
    }

    let domain = &args[1];
    let url = format!(
                "https://web.archive.org/cdx/search/cdx?url=*.{}.
                *&collapse=urlkey&output=text&fl=original",
        domain
        );
    let output = Command::new("curl")
    .arg("-G")
    .arg(&url)
    .arg("--data-urlencode")
    .arg(format!("url=*.{}/*", domain))
    .arg("--data-urlencode")
    .arg("collapse=urlkey")
    .arg("--data-urlencode")
    .arg("output=text")
    .arg("--data-urlencode")
    .arg("fl=original")
    .arg("> out.txt")
    .output()
    .expect("Failed to execute curl command");

    if output.status.success(){
        let mut output_file = std::fs::File::create("out.txt").unwrap();
        std::io::copy(&mut output.stdout.as_slice(), &mut output_file).unwrap();
        println!("Save out.txt");
    } else {
        eprintln!("Error curl: {:?}", output.stderr);
    }
    let client = Client::new();
    // выполняем команду в тз
/*
    let client = Client::builder()
        .timeout(Duration::new(120,0))
        .build()
        .unwrap();
    let url = format!(
        "https://web.archive.org/cdx/search/cdx?url=*.
        {}.*&collapse=urlkey&output=text&fl=original",
        domain
    );
    let response = client.get(&url).send().await.unwrap();
    let body = response.text().await.unwrap();

    let mut output_file = File::create("out.txt").unwrap();
    writeln!(output_file, "{}", body).unwrap();
*/
    let subdomains = extracrt_subdomain("out.txt").await;
    let mut subdomains_file = File::create("subdomains.txt").unwrap();
    for subdomain in subdomains {
        writeln!(subdomains_file, "{}", subdomain).unwrap();
    }

    let screenshots_dir = "screenshots";
    if let Err(_) = fs::create_dir_all(screenshots_dir) {
        eprintln!("Error creat dir screenshots");
        return;
    }
    let js_scripts_dir = "JSscrips";
    if let Err(_) = fs::create_dir_all(js_scripts_dir) {
        eprintln!("Error creat dir JSscrips");
        return;
    }

    let browser = Browser::new(
        LaunchOptionsBuilder::default()
            .headless(true)
            .build()
            .unwrap(),
    )
    .unwrap();

    let urls = read_urls("out.txt").await;
    for url in urls {
        if check_url_200(&url, &client).await {
            println!("Сайт доступен: {}", url);

            // Делаем скриншот
            let screenshot = take_screenshot(&browser, &url).await.unwrap();
            let screenshot_file_name = format!(
                "{}/{}.png",
                screenshots_dir,
                url.replace("/", "_").replace(":", "_")
            );
            let mut screenshot_file = File::create(&screenshot_file_name).unwrap();
            screenshot_file.write_all(&screenshot).unwrap();
            println!("Скриншот сохранен в {}", screenshot_file_name);

            // Сохраняем JavaScript-скрипты
            save_js_scripts(&url, &client, js_scripts_dir).await;
        } else {
            println!("Сайт недоступен: {}", url);
        }
    }

}

async fn extracrt_subdomain(file_path: &str) -> HashSet<String> {
    let mut subdomains = HashSet::new();
    let re = Regex::new(r"https?://([^/]+)/?").unwrap();

    let file = AsyncFile::open(file_path).await.unwrap();
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await.unwrap() {
        if let Some(captures) = re.captures(&line) {
            if let Some(domain) = captures.get(1) {
                subdomains.insert(domain.as_str().to_string());
            }
        }
    }
    subdomains
}

async fn read_urls(file_path: &str) -> Vec<String> {
    let mut urls = Vec::new();

    let file = AsyncFile::open(file_path).await.unwrap();
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await.unwrap() {
        urls.push(line);
    }

    urls
}

async fn check_url_200(url: &str, client: &Client) -> bool {
    match client.get(url).send().await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

async fn take_screenshot(
    browser: &Browser,
    url: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let tab = browser.new_tab().unwrap();
    tab.navigate_to(url)
        .unwrap()
        .wait_until_navigated()
        .unwrap();
    let screenshot = tab
        .capture_screenshot(
            headless_chrome::protocol::page::ScreenshotFormat::PNG,
            None,
            true,
        )
        .unwrap();
    Ok(screenshot)
}

async fn save_js_scripts(url: &str, client: &Client, js_scripts_dir: &str) {
    let response = client.get(url).send().await;
    match response {
        Ok(resp) => {
            let body = resp.text().await.unwrap();
            let document = Html::parse_document(&body);
            let selector = Selector::parse("script[src]").unwrap();

            for element in document.select(&selector) {
                if let Some(src) = element.value().attr("src") {
                    let script_url = if src.starts_with("http") {
                        src.to_string()
                    } else {
                        format!("{}{}", url.trim_end_matches('/'), src)
                    };

                    let script_response = client.get(&script_url).send().await;
                    match script_response {
                        Ok(resp) => {
                            let script_content = resp.text().await.unwrap();

                            let file_name = src.split('/').last().unwrap_or("script.js");
                            let file_path = format!("{}/{}", js_scripts_dir, file_name);
                            let mut file = File::create(&file_path).unwrap();
                            file.write_all(script_content.as_bytes()).unwrap();

                            println!("JavaScript saved in {}", file_path);
                        }
                        Err(e) => {
                            eprintln!("Failed to fetch script from {}: {:?}", script_url, e);
                        }
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to fetch page from {}: {:?}", url, e);
        }
    }
}

