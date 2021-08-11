#![feature(option_result_contains)]

extern crate lazy_static;
extern crate reqwest;
extern crate select;
extern crate serde;

use llh as _;

use chrono::Utc;
use clap::{App, Arg};
use futures::{stream, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::{header::USER_AGENT, Url};
use select::predicate::{Attr, Class, Name, Predicate};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Deserialize)]
struct Criteria {
    #[serde(alias = "ParametricControl")]
    parametric_control: ParametricControl,
}

#[derive(Deserialize)]
struct Results {
    #[serde(alias = "ParametricResults")]
    results: Vec<HashMap<String, serde_json::Value>>,
}

#[derive(Deserialize)]
struct ParametricControl {
    controls: Vec<Control>,
}

#[derive(Deserialize, Clone)]
struct Control {
    id: u32,
    cid: String,
    name: String,
    desc: String,
}

lazy_static::lazy_static! {
    static ref PB_STYLE: ProgressStyle = ProgressStyle::default_bar()
        .template("{msg} {bar:40.magenta/blue} {pos}/{len} ({eta})")
        .progress_chars("##-");
}

#[tokio::main]
async fn main() -> Result<(), reqwest::Error> {
    let mut top: Vec<String> = Vec::new();
    let mut cat_lt = HashSet::new();
    let mut cat_num: HashMap<String, String> = HashMap::new();
    let mut tl: HashMap<String, (String, String)> = HashMap::new();
    let mut db: HashMap<String, HashMap<String, serde_json::Value>> = HashMap::new();

    let matches = App::new("TI Crawler")
        .version("0.1.6")
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

    println!("Start scraping TI at {}", Utc::now());

    if matches.is_present("database")
        && matches
            .values_of("database")
            .unwrap()
            .collect::<Vec<&str>>()
            .contains(&"datasheets")
    {
        println!("Building the database...");
        print!("Parsing main page... ");

        let mut start = Instant::now();

        llh::get_doc("https://www.ti.com")
            .await?
            .find(
                Attr("class", "ti_p-megaMenu-nav-list")
                    .descendant(Name("li").descendant(Name("a"))),
            )
            .filter_map(|n| n.attr("href"))
            .filter(|a| {
                a.starts_with("//") && a.ends_with("overview.html") && !a.contains("/applications/")
            })
            .for_each(|x| top.push(format!("http:{}", x)));

        let mut duration = start.elapsed();
        println!("took {:?}", duration);

        if top.len() == 0 {
            panic!("could not parse page, did the layout change again?");
        }

        let pb = ProgressBar::new(top.len() as u64);
        pb.set_style(PB_STYLE.clone());
        pb.set_message("Parsing menu pages...");

        start = Instant::now();
        for link in top {
            parse_category(&mut cat_lt, link).await?;
            pb.inc(1);
        }
        duration = start.elapsed();
        pb.finish_and_clear();
        println!("Parsing menu pages took {:?}", duration);

        let pb = ProgressBar::new(cat_lt.len() as u64);
        pb.set_style(PB_STYLE.clone());
        pb.set_message("Parsing sub categories...");

        start = Instant::now();

        for link in cat_lt {
            parse_sub_category(&mut cat_num, link).await?;
            pb.inc(1);
        }

        duration = start.elapsed();
        pb.finish_and_clear();
        println!("Parsing sub categories took {:?}", duration);

        let pb = ProgressBar::new(cat_num.len() as u64);
        pb.set_style(PB_STYLE.clone());
        pb.set_message("Fetching criteria information...");

        start = Instant::now();

        for c in &cat_num {
            load_criteria(&mut tl, c.1.clone()).await?;
            pb.inc(1);
        }

        duration = start.elapsed();
        pb.finish_and_clear();
        println!("Fetching criteria information took {:?}", duration);

        let pb = ProgressBar::new(cat_num.len() as u64);
        pb.set_style(PB_STYLE.clone());
        pb.set_message("Fetching results...");
        start = Instant::now();

        for c in &cat_num {
            load_results(&mut db, c.1.clone()).await?;
            pb.inc(1);
        }

        duration = start.elapsed();
        pb.finish_and_clear();
        println!("Fetching results took {:?}", duration);

        // write out the data we produced
        llh::dump_json("json/ti/categories.json", tl.clone());
        llh::dump_json("json/ti/data.json", db.clone());
    }

    if matches.is_present("download")
        && matches
            .values_of("download")
            .unwrap()
            .collect::<Vec<&str>>()
            .contains(&"datasheets")
    {
        let path = Path::new("json/ti/data.json");
        let display = path.display();

        let file = match File::open(&path) {
            Err(why) => panic!("couldn't open {}: {}", display, why),
            Ok(file) => file,
        };

        println!("Loading Database...");

        db = serde_json::from_reader(file).expect("unable to parse db");

        let pb = ProgressBar::new(db.len() as u64);
        pb.set_style(PB_STYLE.clone());
        pb.set_message("Fetching datasheets...");

        let pdfs = stream::iter(db.keys())
            .map(|part| async move {
                llh::save_pdf(
                    format!("https://www.ti.com/lit/gpn/{}", part),
                    format!("pdf/ti/gpn/{}.pdf", part),
                )
                .await
            })
            .buffer_unordered(3);

        pdfs.for_each(|x| async {
            match x {
                Ok(_) => pb.inc(1),
                Err(e) => eprintln!("Got an error: {}", e),
            }
        })
        .await;
    }

    if matches.is_present("database")
        && matches
            .values_of("database")
            .unwrap()
            .collect::<Vec<&str>>()
            .contains(&"techdocs")
    {
        let techdocs = Arc::new(Mutex::new(HashMap::new()));
        let path = Path::new("json/ti/data.json");
        let display = path.display();

        let file = match File::open(&path) {
            Err(why) => panic!("couldn't open {}: {}", display, why),
            Ok(file) => file,
        };

        println!("Loading database...");

        db = serde_json::from_reader(file).expect("unable to parse db");

        let pb = ProgressBar::new(db.len() as u64);
        pb.set_style(PB_STYLE.clone());
        pb.set_message("Fetching part pages...");

        let urls = stream::iter(db.keys())
            .map(|part| async move { load_product_page(part).await })
            .buffer_unordered(3);

        urls.for_each(|x| async {
            match x {
                Ok(m) => {
                    // merge with what we already have
                    let mut techdocs = techdocs.lock().unwrap();
                    techdocs.extend(m);
                    pb.inc(1);
                }
                Err(e) => {
                    eprintln!("Got an error: {}", e);
                }
            }
        })
        .await;

        let db = techdocs.lock().unwrap();
        llh::dump_json("json/ti/techdocs.json", db.clone());
    }

    if matches.is_present("download")
        && matches
            .values_of("download")
            .unwrap()
            .collect::<Vec<&str>>()
            .contains(&"techdocs")
    {
        let path = Path::new("json/ti/techdocs.json");
        let display = path.display();

        let file = match File::open(&path) {
            Err(why) => panic!("couldn't open {}: {}", display, why),
            Ok(file) => file,
        };

        let db: HashMap<String, String> =
            serde_json::from_reader(file).expect("unable to parse db");

        load_techdocs(db).await;
    }

    Ok(())
}

async fn parse_category(cat_lt: &mut HashSet<String>, link: String) -> Result<(), reqwest::Error> {
    let doc = llh::get_doc(link.as_str()).await?;

    doc.find(Class("ti_left-nav-container").descendant(Name("a")))
        .filter_map(|n| n.attr("href"))
        .for_each(|x| {
            cat_lt.insert(String::from(x));
        });

    Ok(())
}

async fn parse_sub_category(
    m: &mut HashMap<String, String>,
    link: String,
) -> Result<(), reqwest::Error> {
    let s = link.replace("overview.html", "products.html");
    let doc = llh::get_doc(s.as_str()).await?;
    let mut category = String::new();

    doc.find(Name("h1"))
        .map(|n| n.text())
        .for_each(|x| category = x);

    category = category
        .replace(" – Products", "")
        .replace(" - Products", "")
        .trim()
        .to_string();

    doc.find(Class("rst"))
        .filter_map(|n| n.attr("familyid"))
        .for_each(|x| {
            m.insert(category.clone(), String::from(x));
        });

    Ok(())
}

async fn load_criteria(
    m: &mut HashMap<String, (String, String)>,
    id: String,
) -> Result<(), reqwest::Error> {
    let client = reqwest::Client::new();
    let url = format!(
        "https://www.ti.com/selectiontool/paramdata/family/{}/criteria?lang=en&output=json",
        id
    );
    let res = client
        .get(Url::parse(url.as_str()).unwrap())
        .header(USER_AGENT, "curl/7.74.0")
        .send()
        .await?
        .json::<Criteria>()
        .await?;

    res.parametric_control.controls.iter().for_each(|c| {
        m.insert(c.cid.clone(), (c.name.clone(), c.desc.clone()));
    });

    Ok(())
}

async fn load_results(
    m: &mut HashMap<String, HashMap<String, serde_json::Value>>,
    id: String,
) -> Result<(), reqwest::Error> {
    let client = reqwest::Client::new();
    let url = format!(
        "https://www.ti.com/selectiontool/paramdata/family/{}/results?lang=en&output=json",
        id
    );
    let res = client
        .get(Url::parse(url.as_str()).unwrap())
        .header(USER_AGENT, "curl/7.74.0")
        .send()
        .await?
        .json::<Results>()
        .await?;

    res.results.iter().for_each(|c| {
        // the key o1 should be there by default, otherwise parsing doesn't make much sense anyways
        let key = c.get("o1").unwrap().as_str().unwrap();

        if m.contains_key(key) {
            let v = m.get(key).unwrap();
            if v.eq(c) {
                return;
            } else {
                if v.len() < c.len() {
                    m.insert(key.to_string(), c.clone());
                } /* else {
                      println!("Duplicate key {} but newer one has less fields", key);
                  } */
            }
        } else {
            m.insert(key.to_string(), c.clone());
        }
    });

    Ok(())
}

async fn load_product_page(id: &String) -> Result<HashMap<String, String>, reqwest::Error> {
    let mut m = HashMap::new();
    let url = format!("https://www.ti.com/product/{}", id);
    llh::get_doc(url.as_str())
        .await?
        .find(Name("ti-techdocs").descendant(Name("a")))
        .filter(|a| {
            let title = a.attr("data-navtitle").unwrap();
            !title.contains("Datasheet") && !title.contains("Data sheet")
        })
        .for_each(|a| {
            m.insert(String::from(a.attr("href").unwrap()), a.text());
        });

    Ok(m)
}

async fn load_techdocs(db: HashMap<String, String>) {
    // filter out only the lit pdfs for now
    let keys: HashSet<String> = db
        .keys()
        .filter(|key| key.starts_with("/lit/pdf"))
        .map(|key| String::from(key))
        .collect();

    let pb = ProgressBar::new(keys.len() as u64);
    pb.set_style(PB_STYLE.clone());
    pb.set_message("Fetching techdocs...");

    let pdfs = stream::iter(keys)
        .map(|doc| async move {
            llh::save_pdf(
                format!("https://www.ti.com{}", doc),
                format!("pdf/ti/lit/{}.pdf", doc.replace("/lit/pdf/", "")),
            )
            .await
        })
        .buffer_unordered(3);

    pdfs.for_each(|x| async {
        match x {
            Ok(_) => pb.inc(1),
            Err(e) => eprintln!("Got an error: {}", e),
        }
    })
    .await;
}
