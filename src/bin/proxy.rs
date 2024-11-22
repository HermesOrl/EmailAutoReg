use reqwest::Error;
use scraper::{Html, Selector};
use std::collections::HashSet;
use tokio;
use serde::{Serialize, Deserialize};
use std::hash::{Hash, Hasher};
use futures::future::BoxFuture;
use futures::future::join_all;

// Структура для хранения информации о прокси
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Proxy {
    ip: String,
    port: String,
    code: String,
    country: String,
    anonymity: String,
    https: String,
}

impl Hash for Proxy {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.ip.hash(state);
        self.port.hash(state);
    }
}
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};
async fn is_proxy_valid(proxy: &Proxy) -> bool {
    let address = format!("{}:{}", proxy.ip, proxy.port);
    // Устанавливаем таймаут 5 секунд для попытки подключения
    match timeout(Duration::from_secs(5), TcpStream::connect(&address)).await {
        Ok(Ok(_stream)) => true,  // Соединение успешно установлено
        _ => false,               // Ошибка подключения или таймаут
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Список функций для сбора прокси
    let fetch_functions: Vec<fn() -> BoxFuture<'static, Result<Vec<Proxy>, Error>>> = vec![
        || Box::pin(fetch_proxies("https://free-proxy-list.net/")),
        || Box::pin(fetch_proxies("https://www.sslproxies.org/")),
        || Box::pin(fetch_proxies("https://www.us-proxy.org/")),
        || Box::pin(fetch_proxies_free_proxy_cz()),
        || Box::pin(fetch_proxies_proxyscrape()),
        || Box::pin(fetch_proxies_openproxy_space()),
        // Добавьте другие функции парсинга здесь
    ];

    let mut proxies = HashSet::new();

    // Асинхронно отправляем запросы к каждому источнику
    let fetches = futures::future::join_all(
        fetch_functions.iter().map(|fetch_fn| fetch_fn())
    );

    let results = fetches.await;

    for result in results {
        match result {
            Ok(list) => {
                for proxy in list {
                    proxies.insert(proxy);
                }
            }
            Err(e) => eprintln!("Ошибка при получении прокси: {}", e),
        }
    }

    // Преобразуем HashSet в Vec для удобств
    let proxy_list: Vec<_> = proxies.into_iter().collect();

    let validation_futures = proxy_list.iter().map(|proxy| is_proxy_valid(proxy));
    let validation_results = join_all(validation_futures).await;

    let valid_proxies: Vec<_> = proxy_list
        .into_iter()
        .zip(validation_results)
        .filter(|(_, is_valid)| *is_valid)
        .map(|(proxy, _)| proxy)
        .collect();
    for proxy in &valid_proxies {
        println!(
            "{}:{} - {} - {} - {} - HTTPS: {}",
            proxy.ip, proxy.port, proxy.code, proxy.country, proxy.anonymity, proxy.https
        );
    }

    // (Опционально) Сохраняем прокси в файл JSON
    /*
    let json = serde_json::to_string_pretty(&proxy_list)?;
    std::fs::write("proxies.json", json)?;
    */

    Ok(())
}

// Универсальная функция для получения прокси с сайтов с таблицами
async fn fetch_proxies(url: &str) -> Result<Vec<Proxy>, Error> {
    let response = reqwest::get(url).await?.text().await?;
    let document = Html::parse_document(&response);

    // CSS-селектор для таблицы с прокси
    let selector = Selector::parse("table tbody tr").unwrap();

    let mut proxies = Vec::new();

    for row in document.select(&selector) {
        let cols: Vec<_> = row.select(&Selector::parse("td").unwrap()).collect();
        if cols.len() < 7 {
            continue; // Пропустить строки с недостаточным количеством колонок
        }

        let proxy = Proxy {
            ip: cols[0].text().collect::<Vec<_>>().join("").trim().to_string(),
            port: cols[1].text().collect::<Vec<_>>().join("").trim().to_string(),
            code: cols[2].text().collect::<Vec<_>>().join("").trim().to_string(),
            country: cols[3].text().collect::<Vec<_>>().join("").trim().to_string(),
            anonymity: cols[4].text().collect::<Vec<_>>().join("").trim().to_string(),
            https: cols[6].text().collect::<Vec<_>>().join("").trim().to_string(),
        };

        proxies.push(proxy);
    }

    Ok(proxies)
}

// Функция для получения списка прокси с free-proxy.cz
async fn fetch_proxies_free_proxy_cz() -> Result<Vec<Proxy>, Error> {
    let url = "https://free-proxy.cz/en/proxylist/country/all/";
    let response = reqwest::get(url).await?.text().await?;
    let document = Html::parse_document(&response);

    // CSS-селектор для таблицы с прокси
    let selector = Selector::parse("table#proxy_list tbody tr").unwrap();

    let mut proxies = Vec::new();

    for row in document.select(&selector) {
        let cols: Vec<_> = row.select(&Selector::parse("td").unwrap()).collect();
        if cols.len() < 7 {
            continue; // Пропустить строки с недостаточным количеством колонок
        }

        let ip = cols[0].text().collect::<Vec<_>>().join("").trim().to_string();
        let port = cols[1].text().collect::<Vec<_>>().join("").trim().to_string();
        let code = cols[2].text().collect::<Vec<_>>().join("").trim().to_string();
        let country = cols[3].text().collect::<Vec<_>>().join("").trim().to_string();
        let anonymity = cols[4].text().collect::<Vec<_>>().join("").trim().to_string();
        let https = cols[6].text().collect::<Vec<_>>().join("").trim().to_string();

        proxies.push(Proxy {
            ip,
            port,
            code,
            country,
            anonymity,
            https,
        });
    }

    Ok(proxies)
}

// Функция для получения списка прокси с proxyscrape.com
async fn fetch_proxies_proxyscrape() -> Result<Vec<Proxy>, Error> {
    let url = "https://api.proxyscrape.com/v2/?request=displayproxies&protocol=http&timeout=10000&country=all&ssl=all&anonymity=all";
    let response = reqwest::get(url).await?.text().await?;
    let mut proxies = Vec::new();

    for line in response.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() != 2 {
            continue;
        }
        let proxy = Proxy {
            ip: parts[0].to_string(),
            port: parts[1].to_string(),
            code: "".to_string(),
            country: "".to_string(),
            anonymity: "".to_string(),
            https: "".to_string(),
        };
        proxies.push(proxy);
    }

    Ok(proxies)
}

// Функция для получения списка прокси с openproxy.space
async fn fetch_proxies_openproxy_space() -> Result<Vec<Proxy>, Error> {
    let url = "https://openproxy.space/list/http";
    let response = reqwest::get(url).await?.text().await?;
    let document = Html::parse_document(&response);

    // CSS-селектор для таблицы с прокси
    let selector = Selector::parse("table tbody tr").unwrap();

    let mut proxies = Vec::new();

    for row in document.select(&selector) {
        let cols: Vec<_> = row.select(&Selector::parse("td").unwrap()).collect();
        if cols.len() < 7 {
            continue;
        }

        let ip = cols[0].text().collect::<Vec<_>>().join("").trim().to_string();
        let port = cols[1].text().collect::<Vec<_>>().join("").trim().to_string();
        let code = cols[2].text().collect::<Vec<_>>().join("").trim().to_string();
        let country = cols[3].text().collect::<Vec<_>>().join("").trim().to_string();
        let anonymity = cols[4].text().collect::<Vec<_>>().join("").trim().to_string();
        let https = cols[6].text().collect::<Vec<_>>().join("").trim().to_string();

        proxies.push(Proxy {
            ip,
            port,
            code,
            country,
            anonymity,
            https,
        });
    }

    Ok(proxies)
}

// Добавьте аналогичные функции для других источников...
