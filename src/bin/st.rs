extern crate reqwest;
extern crate select;
extern crate serde;

use llh as _;

use chrono::Utc;
use futures::{stream, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use select::predicate::{Attr, Class};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::path::Path;
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
    let pb_style = ProgressStyle::default_bar()
        .template("{msg} {bar:40.magenta/blue} {pos}/{len} ({eta})")
        .progress_chars("##-");

    println!("Start scraping ST at {}", Utc::now());
    print!("Fetching main page... ");

    let mut pages: Vec<String> = Vec::new();
    let mut data_pages: HashMap<String, String> = HashMap::new();

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
    pb.set_style(pb_style.clone());
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
    pb.set_style(pb_style.clone());
    pb.set_message("Fetching all data-pages...");

    let start = Instant::now();

    // TODO: save all the JSON data into a database
    for p in &data_pages {
        llh::save_json(p.1.to_string(), format!("json/st/{}.json", p.0)).await?;
        pb.inc(1);
    }

    let duration = start.elapsed();
    pb.finish_and_clear();
    println!("Fetching all data-pages took {:?}", duration);

    let mpb = ProgressBar::new(data_pages.len() as u64);
    mpb.set_style(pb_style.clone());
    mpb.set_message("Fetching datasheets...");

    let start = Instant::now();

    // parse the files again and download the datasheets (where available)
    for p in &data_pages {
        let filename = format!("json/st/{}.json", p.0);
        let path = Path::new(&filename);
        let display = path.display();
        let file = match File::open(path) {
            Err(why) => panic!("couldn't create {}: {}", display, why),
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
        pb_inner.set_style(pb_style.clone());
        pb_inner.set_message(&format!("Fetching data-page {}...", p.0));

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
                            format!("pdf/st/{}.pdf", pn),
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

    let duration = start.elapsed();
    mpb.finish_and_clear();
    println!("Fetching datasheets took {:?}", duration);

    Ok(())
}
