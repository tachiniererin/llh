extern crate reqwest;
extern crate select;
extern crate serde;

use llh as _;

use chrono::Utc;
use clap::{App, Arg};
use dashmap::DashMap;
use futures::{stream, StreamExt};
use indicatif::ProgressBar;
use reqwest::{header::USER_AGENT, Url};
use select::document::Document;
use select::predicate::{Attr, Class, Name};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

#[derive(Deserialize)]
#[allow(dead_code)]
struct Category {
    columns: Vec<HashMap<String, serde_json::Value>>,
    rows: Vec<Product>,
    #[serde(alias = "levelTitle")]
    level_title: String,
    breadcrumb: String,
}

#[derive(Deserialize, Clone)]
struct Product {
    #[serde(alias = "productId")]
    product_id: String,
    path: String,
    cells: Vec<HashMap<String, String>>,
    #[serde(alias = "productFolderUrl")]
    product_folder_url: String,
    #[serde(alias = "availableInDistributorStock")]
    available_in_distributor_stock: bool,
    #[serde(alias = "availableAsFreeSample")]
    available_as_free_sample: bool,
    #[serde(alias = "newProductIntroduction")]
    new_product_introduction: bool,
    #[serde(alias = "isNewProduct")]
    is_new_product: bool,
    #[serde(alias = "isPublic")]
    is_public: bool,
}

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    println!("Start scraping ST at {}", Utc::now());
    print!("Fetching main page... ");

    let mut pages: Vec<String> = Vec::new();
    let data_pages = Arc::new(DashMap::new());

    let matches = App::new("ST Crawler")
        .version(llh::VERSION)
        .about("Builds a DB of all the parts and datasheets")
        .arg(
            Arg::with_name("database")
                .short("b")
                .long("database")
                .multiple(true)
                .takes_value(true)
                .help("Build the database, pass datasheets and or techdocs as values"),
        )
        .arg(
            Arg::with_name("download")
                .short("d")
                .long("download")
                .multiple(true)
                .takes_value(true)
                .help("Fetch all the datasheets and/or techdocs"),
        )
        .get_matches();

    if matches.is_present("database")
        && matches
            .values_of("database")
            .unwrap()
            .collect::<Vec<&str>>()
            .contains(&"datasheets")
    {
        let start = Instant::now();

        llh::get_doc("https://www.st.com")
            .await?
            .find(Class("st-nav__blockmenu-link"))
            .filter_map(|n| n.attr("href"))
            .filter(|a| {
                a.starts_with("/en/")
                    && !a.contains("/applications/")
                    && !a.contains("/development-tools/")
                    && !a.contains("/embedded-software/")
                    && !a.contains("/evaluation-tools/")
            })
            .for_each(|x| pages.push(format!("https://www.st.com{}", x)));

        let duration = start.elapsed();
        println!("took {:?}", duration);

        let pb = ProgressBar::new(pages.len() as u64);
        pb.set_style(llh::PB_STYLE.clone());
        pb.set_message("Fetching all sub-categories...");

        let start = Instant::now();

        for page in pages {
            llh::get_doc(page.as_str())
                .await?
                .find(Attr("name", "didyouknow.productId"))
                .filter_map(|n| n.attr("value"))
                .for_each(|x| {
                    let link = String::from(format!(
                        "{}.cxst-ps-grid.html/{}.json",
                        page.replace(".html", ""),
                        x
                    ));
                    data_pages.insert(String::from(x), link);
                });
            pb.inc(1);
        }

        let duration = start.elapsed();
        pb.finish_and_clear();
        println!("Fetching all sub-categories took {:?}", duration);

        let pb = ProgressBar::new(data_pages.len() as u64);
        pb.set_style(llh::PB_STYLE.clone());
        pb.set_message("Fetching all data-pages...");

        let start = Instant::now();

        // TODO: save all the JSON data into a database
        for p in data_pages.iter() {
            llh::save_json(
                p.value().to_string(),
                format!("json/st/datapages/{}.json", p.key()),
            )
            .await?;
            pb.inc(1);
        }

        let duration = start.elapsed();
        pb.finish_and_clear();
        println!("Fetching all data-pages took {:?}", duration);
    } else {
        let paths = fs::read_dir("json/st/datapages/").unwrap();

        for path in paths {
            if path.is_err() {
                continue;
            }
            let file = path.unwrap().file_name();
            let key = file.to_str().unwrap().trim_end_matches(".json");

            data_pages.insert(key.to_string(), String::from(""));
        }
    }

    if matches.is_present("database")
        && matches
            .values_of("database")
            .unwrap()
            .collect::<Vec<&str>>()
            .contains(&"techdocs")
    {
        let mpb = ProgressBar::new(data_pages.len() as u64);
        mpb.set_style(llh::PB_STYLE.clone());
        mpb.set_message("Fetching techdoc db...");

        let start = Instant::now();

        parse_product_documentation(&mpb, data_pages.as_ref()).await;

        let duration = start.elapsed();
        mpb.finish_and_clear();
        println!("Fetching techdoc database took {:?}", duration);
    }

    if matches.is_present("download")
        && matches
            .values_of("download")
            .unwrap()
            .collect::<Vec<&str>>()
            .contains(&"datasheets")
    {
        let mpb = ProgressBar::new(data_pages.len() as u64);
        mpb.set_style(llh::PB_STYLE.clone());
        mpb.set_message("Fetching datasheets...");

        let start = Instant::now();

        parse_product_folders(&mpb, &data_pages).await;

        let duration = start.elapsed();
        mpb.finish_and_clear();
        println!("Fetching datasheets took {:?}", duration);
    }

    if matches.is_present("download")
        && matches
            .values_of("download")
            .unwrap()
            .collect::<Vec<&str>>()
            .contains(&"techdocs")
    {
        let filename = "json/st/techdocs.json";
        let path = Path::new(&filename);
        let display = path.display();
        let file = match File::open(path) {
            Err(why) => panic!("could not open {}: {}", display, why),
            Ok(file) => file,
        };

        let cat: HashMap<String, String> = match serde_json::from_reader(file) {
            Err(why) => {
                panic!("could not parse {}: {}", display, why);
            }
            Ok(v) => v,
        };

        let mpb = ProgressBar::new(cat.len() as u64);
        mpb.set_style(llh::PB_STYLE.clone());
        mpb.set_message("Fetching techdocs...");

        let start = Instant::now();

        let pdfs = stream::iter(cat)
            .map(|p| async move {
                llh::save_pdf(
                    format!("https://www.st.com{}", p.1),
                    format!("pdf/st/techdocs/{}.pdf", p.0.replace("/", "_")),
                )
                .await
            })
            .buffer_unordered(8);

        pdfs.for_each(|x| async {
            match x {
                Ok(_) => mpb.inc(1),
                Err(e) => eprintln!("Got an error: {}", e),
            }
        })
        .await;

        let duration = start.elapsed();
        mpb.finish_and_clear();
        println!("Fetching techdocs took {:?}", duration);
    }

    Ok(())
}

async fn parse_product_folders(mpb: &ProgressBar, data_pages: &DashMap<String, String>) {
    // parse the files again and download the datasheets (where available)
    for p in data_pages {
        let filename = format!("json/st/datapages/{}.json", p.key());
        let path = Path::new(&filename);
        let display = path.display();
        let file = match File::open(path) {
            Err(why) => panic!("could not open {}: {}", display, why),
            Ok(file) => file,
        };

        // try parsing it, or skip if it can't be parsed
        let cat: Category = match serde_json::from_reader(file) {
            Err(why) => {
                println!("could not parse {}: {}", display, why);
                continue;
            }
            Ok(v) => v,
        };

        let pb_inner = ProgressBar::new(cat.rows.len() as u64);
        pb_inner.set_style(llh::PB_STYLE.clone());
        pb_inner.set_message(format!("Fetching data-page {}...", p.key()));

        let pdfs = stream::iter(cat.rows)
            .map(|p| {
                let mut pn: String = String::from("");
                for c in p.cells {
                    if c["columnId"] == "1" {
                        pn = c["value"].clone();
                        break;
                    }
                }
                async move {
                    if pn.len() == 0 {
                        llh::empty().await
                    } else {
                        llh::save_pdf(
                            format!("https://www.st.com/resource/en/datasheet/{}.pdf", pn),
                            format!("pdf/st/datasheets/{}.pdf", pn),
                        )
                        .await
                    }
                }
            })
            .buffer_unordered(8);

        pdfs.for_each(|x| async {
            match x {
                Ok(_) => pb_inner.inc(1),
                Err(e) => eprintln!("Got an error: {}", e),
            }
        })
        .await;

        pb_inner.finish_and_clear();
        mpb.inc(1);
    }
}

async fn parse_product_documentation(mpb: &ProgressBar, data_pages: &DashMap<String, String>) {
    let techdocs = Arc::new(DashMap::new());

    // parse the files again and download the datasheets (where available)
    for p in data_pages {
        let filename = format!("json/st/datapages/{}.json", p.key());
        let path = Path::new(&filename);
        let display = path.display();
        let file = match File::open(path) {
            Err(why) => panic!("could not open {}: {}", display, why),
            Ok(file) => file,
        };

        // try parsing it, or skip if it can't be parsed
        let cat: Category = match serde_json::from_reader(file) {
            Err(why) => {
                println!("could not parse {}: {}", display, why);
                continue;
            }
            Ok(v) => v,
        };

        let pb_inner = ProgressBar::new(cat.rows.len() as u64);
        pb_inner.set_style(llh::PB_STYLE.clone());
        pb_inner.set_message(format!("Fetching data-page {}...", p.key()));

        let pdfs = stream::iter(cat.rows)
            .map(|p| {
                let folder = p.product_folder_url;
                async move {
                    // TODO: parse product folder
                    get_doc_sdi(
                        format!("https://www.st.com{}", folder).as_str(),
                        "design-resources.html",
                    )
                    .await
                }
            })
            .buffer_unordered(8);

        pdfs.for_each(|x| async {
            match x {
                Ok(doc) => {
                    doc.find(Name("span"))
                        .filter(|n| n.attr("data-translation-app-exclude").is_some())
                        .for_each(|n| {
                            let key = n.text();
                            let value = n.parent().unwrap().attr("href").unwrap();

                            if key.len() > 0 && !value.contains("/datasheet/") {
                                techdocs.insert(key.trim().to_string(), String::from(value.trim()));
                                // eprintln!("adding: {}:{}", key.trim(), value.trim());
                            }
                        });
                    pb_inner.inc(1)
                }
                Err(e) => eprintln!("Got an error: {}", e),
            }
        })
        .await;

        pb_inner.finish_and_clear();
        mpb.inc(1);
    }

    llh::dump_json("json/st/techdocs.json", techdocs.as_ref());
}

// get_doc_sdi follows the link and looks for an SDI include comment of the specified type
async fn get_doc_sdi(link: &str, typ: &str) -> Result<Document, reqwest::Error> {
    let client = reqwest::Client::new();
    let res = client
        .get(Url::parse(link).unwrap())
        .header(USER_AGENT, "curl/7.74.0")
        .send()
        .await?;

    let body = res.text().await?;

    let mut new_link = "";

    for line in body.split("\n") {
        let l = line.trim_start();
        if l.starts_with("<!-- SDI include") {
            if l.contains(typ) {
                for part in l.split(" ") {
                    if part.starts_with("/") {
                        new_link = part.strip_suffix(",").unwrap_or(part);
                        break;
                    }
                }
            }
        }
    }

    let url = format!("https://www.st.com{}", new_link);

    let res = client
        .get(Url::parse(url.as_str()).unwrap())
        .header(USER_AGENT, "curl/7.74.0")
        .send()
        .await?;

    let body = res.text().await?;

    Ok(Document::from(body.as_str()))
}
