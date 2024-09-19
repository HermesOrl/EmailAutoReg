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
    let mut retries = 3;

    while retries > 0 {
        // Get the next proxy
        let proxy = {
            let mut pm = proxy_manager.lock().unwrap();
            pm.get_next_proxy()
        };

        // Build the client with proxy
        let proxy_url = reqwest::Proxy::all(&proxy)?;
        let client = Client::builder()
            .proxy(proxy_url)
            .timeout(Duration::from_secs(20))
            .build()?;

        // Generate random email and password
        let email = format!("{}@starmail.net", random_string());
        let password = random_string();

        println!("Email: {}, Password: {}\tProxy -> {}", email, password, proxy);

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
                println!("Status code: {}", status);
                if status == 201 {
                    // Write to file
                    let line = format!("{}:{}", email, password);
                    file_writer.write_line(&line)?;
                    return Ok(());
                } else {
                    eprintln!("Error: Status code not 201");
                    retries -= 1;
                }
            }
            Err(e) => {
                eprintln!("Error sending request: {}", e);
                retries -= 1;
            }
        }

        if retries > 0 {
            // Wait before retrying
            sleep(Duration::from_millis(100)).await;
        }
    }

    Err("Failed to create account after retries".into())
}

async fn worker(
    proxy_manager: Arc<Mutex<ProxyManager>>,
    file_writer: Arc<FileWriter>,
    semaphore: Arc<Semaphore>,
) {
    let permit = semaphore.acquire_owned().await.unwrap();

    // Each worker creates 10 accounts
    for _ in 0..10 {
        if let Err(e) = create_account(proxy_manager.clone(), file_writer.clone()).await {
            eprintln!("Error creating account: {}", e);
            // Return early if error occurs
            return;
        }
    }

    drop(permit); // Release the permit
}

#[tokio::main]
async fn main() {
    let proxies = read_proxies_from_file("proxy.txt").expect("Failed to read proxies from file");
    let proxy_manager = Arc::new(Mutex::new(ProxyManager::new(proxies)));
    let file_writer = Arc::new(FileWriter::new("FREE_EMAILS.txt").expect("Failed to open file for writing"));

    let num_jobs = 40000;
    let max_concurrent_tasks = 1000;
    let semaphore = Arc::new(Semaphore::new(max_concurrent_tasks));

    let num_workers = num_jobs / 10;
    let mut handles = Vec::new();

    for _ in 0..num_workers {
        let proxy_manager = proxy_manager.clone();
        let file_writer = file_writer.clone();
        let semaphore = semaphore.clone();

        let handle = tokio::spawn(async move {
            worker(proxy_manager, file_writer, semaphore).await;
        });

        handles.push(handle);

        // Optionally, add a small sleep
        // sleep(Duration::from_nanos(5)).await;
    }

    futures::future::join_all(handles).await;
}
