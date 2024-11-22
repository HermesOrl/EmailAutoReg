use std::env;
use rand::{thread_rng, Rng};
use rand::distributions::Alphanumeric;
use reqwest::Client;
use serde_json::json;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;
use tokio::time::{sleep, Duration};
use futures::future::join_all;

fn random_string() -> String {
    thread_rng().sample_iter(&Alphanumeric).take(10).map(char::from).collect()
}

struct ProxyManager {
    proxies: Vec<String>,
    index: usize,
}

impl ProxyManager {
    fn new(proxies: Vec<String>) -> Self {
        ProxyManager { proxies, index: 0 }
    }

    fn get_next_proxy(&mut self) -> String {
        let proxy = self.proxies[self.index % self.proxies.len()].clone();
        self.index += 1;
        proxy
    }
}

fn read_proxies_from_file<P: AsRef<Path>>(path: P) -> std::io::Result<Vec<String>> {
    let file = File::open(path)?;
    let buf_reader = BufReader::new(file);
    let proxies = buf_reader.lines().collect::<Result<Vec<_>, _>>()?;
    Ok(proxies)
}

struct FileWriter {
    file: Arc<Mutex<File>>,
}
use scraper::{Html, Selector};
use std::error::Error;

async fn fetch_proxies_from_web() -> Result<Vec<String>, Box<dyn Error>> {
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (compatible; ProxyFetcher/1.0)")
        .build()?;

    let response = client
        .get("https://free-proxy-list.net/")
        .send()
        .await?
        .text()
        .await?;

    let document = Html::parse_document(&response);
    let row_selector = Selector::parse("div.table-responsive.fpl-list table tbody tr")?;
    let td_selector = Selector::parse("td")?;
    let mut proxies = Vec::new();

    for element in document.select(&row_selector) {
        let columns: Vec<_> = element.select(&td_selector).collect();

        if columns.len() < 7 {
            // Пропускаем строки с недостаточным количеством столбцов
            continue;
        }

        let ip = columns[0].inner_html().trim().to_string();
        let port = columns[1].inner_html().trim().to_string();
        let https = columns[6].inner_html().trim().to_lowercase(); // 7-й столбец (индекс 6)

        // Формируем строку прокси в зависимости от поддержки HTTPS
        let proxy = if https == "yes" {
            format!("https://{}:{}", ip, port)
        } else {
            format!("http://{}:{}", ip, port)
        };

        proxies.push(proxy);
    }

    println!("Найдено {} прокси.", proxies.len()); // Для отладки

    Ok(proxies)
}

use tokio::time::{timeout};
use tokio::net::TcpStream;

async fn is_proxy_working_async(proxy: &str, timeout_secs: u64) -> bool {
    let timeout_duration = Duration::from_secs(timeout_secs);
    let connect_future = TcpStream::connect(proxy);
    match timeout(timeout_duration, connect_future).await {
        Ok(Ok(_stream)) => true,
        _ => false,
    }
}

impl FileWriter {
    fn new<P: AsRef<Path>>(path: P) -> std::io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        Ok(FileWriter { file: Arc::new(Mutex::new(file)) })
    }

    fn write_line(&self, line: &str) -> std::io::Result<()> {
        let mut file = self.file.lock().unwrap();
        writeln!(file, "{}", line)?;
        Ok(())
    }
}

async fn create_account(
    proxy_manager: Arc<Mutex<ProxyManager>>,
    file_writer: Arc<FileWriter>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut retries = 1;

    while retries > 0 {
        // Get the next proxy
        let proxy = {
            let mut pm = proxy_manager.lock().unwrap();
            pm.get_next_proxy()
        };

        let proxy_url = reqwest::Proxy::all(&proxy)?;
        let client = Client::builder()
            // .proxy(proxy_url)
            .timeout(Duration::from_secs(10))
            .build()?;

        // Generate random email and password
        let email = format!("{}@livinitlarge.net", random_string());
        let password = random_string();

        // println!("Email: {}, Password: {}\tProxy -> {}", email, password, proxy);

        // Prepare data
        let data = json!({
            "address": email,
            "password": password,
        });

        // Send the POST request
        let res = client
            .post("https://api.mail.tm/accounts")
            .json(&data)
            .send()
            .await;

        match res {
            Ok(response) => {
                let status = response.status();
                // println!("Status code: {}", status);
                if status == 201 {
                    // Write to file
                    let line = format!("{}:{}", email, password);
                    file_writer.write_line(&line)?;
                    return Ok(());
                } else {
                    // eprintln!("Error: Status code not 201");
                    retries -= 1;
                }
            }
            Err(e) => {
                // eprintln!("Error sending request: {}", e);
                retries -= 1;
            }
        }

        if retries > 0 {
            sleep(Duration::from_millis(5)).await;
        }
    }

    Err("Failed to create account after retries".into())
}

async fn worker(
    proxy_manager: Arc<Mutex<ProxyManager>>,
    file_writer: Arc<FileWriter>,
    rps_counter: Arc<AtomicUsize>
) {
    // Each worker creates 10 accounts
    for _ in 0..1 {
        if let Err(e) = create_account(proxy_manager.clone(), file_writer.clone()).await {
            // eprintln!("Error creating account: {}", e);
            // return;
        }
        rps_counter.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(tests)]
use crate::*;

async fn monitor_rps(totalrequests: Arc<AtomicUsize>) {
    let starttime = tokio::time::Instant::now();
    let mut interval = interval(Duration::from_secs(1));
    loop {
        interval.tick().await;
        let elapsed = starttime.elapsed();
        let elapsedsecs = elapsed.as_secs_f64();
        let total = totalrequests.load(Ordering::Relaxed);
        let rps = total as f64 / elapsedsecs;
        println!("Накопительный RPS: {:.2}", rps);
    }
}

#[tokio::test]
async fn test_fetch_proxy() {
    let proxies = fetch_proxies_from_web().await;
    match proxies {
        Ok(value) => {println!("Proxies:\n{:?}", value)}
        Err(err) => {eprintln!("Error fetch proxy -> {}", err)}
    }
}
use std::sync::atomic::{AtomicUsize, Ordering};
use futures::StreamExt;
use tokio::time::{interval};
#[tokio::main(flavor = "multi_thread", worker_threads=4)]
async fn main() {



    let proxies = read_proxies_from_file("proxy.txt").expect("Failed to read proxies from file");
    let proxy_manager = Arc::new(Mutex::new(ProxyManager::new(proxies)));
    let file_writer = Arc::new(FileWriter::new("FREE_EMAILS.txt").expect("Failed to open file for writing"));

    let num_jobs = 20_000;
    let max_concurrent_tasks =
        env::var("THREADS").unwrap_or_else(|_| "30".to_string());
    let semaphore = Arc::new(Semaphore::new(max_concurrent_tasks.parse().unwrap()));

    let mut tasks = futures::stream::FuturesUnordered::new();

    let totalrequests = Arc::new(AtomicUsize::new(0));
    let totalrequestsclone = totalrequests.clone();
    let monitorhandle = tokio::spawn(async move {
        monitor_rps(totalrequestsclone).await;
    });

    for _ in 0..num_jobs {
        let proxy_manager = proxy_manager.clone();
        let file_writer = file_writer.clone();
        let semaphore = Arc::clone(&semaphore);
        let totalrequests = totalrequests.clone();
        let handle = tokio::spawn(async move {
            let permit = match semaphore.acquire_owned().await {
                Ok(p) => p,
                Err(e) => {
                    println!("Не удалось захватить семафор: {:?}", e);
                    return;
                }
            };
            worker(proxy_manager, file_writer, totalrequests).await;
        });

        tasks.push(handle);

        // Optionally, add a small sleep
        // sleep(Duration::from_nanos(5)).await;
    }

    while let Some(res) = tasks.next().await {
        match res {
            Ok(_) => {}
            Err(e) => {
               println!("Task in process_file failed: {:?}", e);
            }
        }
    }
}
