mod api;
mod database;

use std::{collections::BTreeSet, time::Instant};

use api::DomainBlock;
use color_eyre::Result;
use database::{DatabaseAccess, DatabaseInstance, DatabaseObject};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const DATABASE_FILE: &str = "database.json";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async {
            if let Err(e) = real_main().await {
                panic!("Error in main: {e}");
            }
        });

    Ok(())
}

async fn real_main() -> Result<()> {
    let arg = std::env::args().collect::<Vec<_>>();
    if arg.len() < 2 {
        println!("Available verbs: fetch, process, crack, show");
        return Ok(());
    }

    let mut db = DatabaseInstance::load(DATABASE_FILE)
        .unwrap_or_default()
        .use_namespace("mastodon-blocks");

    match arg.get(1).unwrap().as_str() {
        "fetch" => {
            println!("Loading blocklist from seed domains");
            try_load_blocklist(&mut db, "mastodon.social").await;
            // try_load_blocklist(&mut db, "pawoo.net").await;
            try_load_blocklist(&mut db, "mstdn.jp").await;
            try_load_blocklist(&mut db, "mastodon.cloud").await;
            try_load_blocklist(&mut db, "mastodon.online").await;
            // try_load_blocklist(&mut db, "counter.social").await;
            try_load_blocklist(&mut db, "mstdn.social").await;
            try_load_blocklist(&mut db, "mas.to").await;
            // try_load_blocklist(&mut db, "gc2.jp").await;
            // try_load_blocklist(&mut db, "mastodon.world").await;
            try_load_blocklist(&mut db, "home.social").await;

            println!("Updating database");
            process_db(&mut db)?;
        }
        "process" => {
            println!("Updating database");
            process_db(&mut db)?;
        }
        "crack" => {
            crack(&mut db)?;
        }
        "show" => {
            show(&mut db)?;
        }
        verb => {
            println!("Unknown verb: {verb}");
        }
    }

    db.pop_namespace().save(DATABASE_FILE)?;

    Ok(())
}

async fn try_load_blocklist(db: &mut DatabaseAccess, domain: &str) {
    let err = load_blocklist(db, domain).await;

    if let Err(e) = err {
        println!("Error while trying to load blocklist from {domain}: {e}");
    }
}

async fn load_blocklist(db: &mut DatabaseAccess, domain: &str) -> Result<()> {
    let client = reqwest::Client::new();

    // mstdn.jp requires a user agent or will serve a 404
    let val: Vec<DomainBlock> = client
        .get(format!("https://{domain}/api/v1/instance/domain_blocks"))
        .header("user-agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/112.0.0.0 Safari/537.36")
        .send()
        .await?
        .json()
        .await?;

    println!("Loaded {} blocklist items from {domain}", val.len());

    db.set(MastodonBlockList {
        domain: domain.to_string(),
        list: val,
    });

    Ok(())
}

fn process_db(db: &mut DatabaseAccess) -> Result<()> {
    let things = db.iter_keys::<MastodonBlockList>().collect::<Vec<_>>();
    for thing in things {
        let item = db.get::<MastodonBlockList>(&thing)?.unwrap();
        for blocked_item in item.list {
            // TODO: should update instead
            let mut domain: DomainEntry = blocked_item.try_into()?;

            if let Some(existing) = db.get::<DomainEntry>(&domain.get_id())? {
                domain = domain.merge(existing);
            }

            db.set(domain);
        }
    }

    Ok(())
}

fn crack(db: &mut DatabaseAccess) -> Result<()> {
    let keys = db.iter_keys::<DomainEntry>().collect::<Vec<_>>();
    let mut entries = keys
        .into_iter()
        .map(|k| db.get::<DomainEntry>(&k).unwrap().unwrap())
        .collect::<Vec<_>>();
    let num_total = entries.len();
    entries.retain(|x| x.known_domain.is_none());
    println!(
        "Found {}/{} entries with no fully known domain",
        entries.len(),
        num_total
    );

    // TODO: merge domains where multiple partial domains are known

    entries.sort_by_key(|x| {
        x.partial_domains
            .iter()
            .map(|d| d.chars().filter(|c| *c == '*').count())
            .min()
    });

    for entry in &entries {
        for d in &entry.partial_domains {
            println!("{}: {d}", entry.get_id());
            let now = Instant::now();
            let found = brute_force(d, entry.digest);
            let elapsed = Instant::now() - now;
            println!("> Found: {found:?} in {elapsed:?}");

            if let Some(found) = found {
                let mut domain = db.get::<DomainEntry>(&entry.get_id())?.unwrap();
                domain.known_domain = Some(found);
                db.set(domain);

                // TODO: not ideal
                db.clone().pop_namespace().save(DATABASE_FILE)?;
            }
        }
    }

    Ok(())
}

fn brute_force(pattern: &str, expected_digest: [u8; 32]) -> Option<String> {
    // TODO: we can narrow down the TLD, there is no need to brute-force that
    if pattern.len() > 32 {
        panic!("url {pattern} too long");
    }

    let buffer_len = pattern.len();

    const ALPHABET: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let wildcard_count = pattern.chars().filter(|c| *c == '*').count();

    let total_count = ALPHABET.len().pow(wildcard_count as u32);
    // println!("Brute-force attempt count for {pattern} is {total_count}");

    // (0..total_count).find_map(|i| {
    (0..total_count).into_par_iter().find_map_any(|i| {
        let mut buffer = [0u8; 32];
        let buffer = &mut buffer[..buffer_len];
        buffer.copy_from_slice(pattern.as_bytes());

        for wc_idx in 0..wildcard_count {
            let x = i / ALPHABET.len().pow(wc_idx as u32);
            let x = x % ALPHABET.len();

            let char_to_place = ALPHABET[x];
            let (char_index, _) = buffer
                .iter()
                .enumerate()
                .find(|(_, b)| **b == b'*')
                .unwrap();
            buffer[char_index] = char_to_place;
        }
        // println!("iteration {i}: {}", String::from_utf8_lossy(buffer));

        let mut hasher = Sha256::new();
        hasher.update(&buffer);
        let found_digest = hasher.finalize();
        // let found_digest = sha256::digest(buffer.as_ref());

        if found_digest[..] == expected_digest {
            Some(String::from_utf8_lossy(buffer).to_string())
        } else {
            None
        }
    })
}

fn show(db: &mut DatabaseAccess) -> Result<()> {
    let entries = db
        .iter_keys::<DomainEntry>()
        .collect::<Vec<_>>()
        .into_iter()
        .map(|k| db.get::<DomainEntry>(&k).unwrap().unwrap())
        .collect::<Vec<_>>();

    let blocklists = db
        .iter_keys::<MastodonBlockList>()
        .collect::<Vec<_>>()
        .into_iter()
        .map(|k| db.get::<MastodonBlockList>(&k).unwrap().unwrap())
        .collect::<Vec<_>>();

    for entry in &entries {
        let display_domain = entry
            .known_domain
            .clone()
            .unwrap_or_else(|| entry.partial_domains.first().unwrap().clone());
        println!("{display_domain}");

        // find which domains block this one
        for blocklist in &blocklists {
            if let Some(blocklist_entry) =
                blocklist.list.iter().find(|e| e.digest == entry.get_id())
            {
                if let Some(reason) = &blocklist_entry.comment {
                    println!("- Blocked by {} for reason: {reason}", blocklist.domain);
                } else {
                    println!("- Blocked by {}", blocklist.domain);
                }
            }
        }

        println!();
    }

    Ok(())
}

#[derive(Serialize, Deserialize)]
struct MastodonBlockList {
    pub domain: String,
    pub list: Vec<DomainBlock>,
}

impl DatabaseObject for MastodonBlockList {
    const KEY_NAME: &'static str = "blocklist";

    fn get_id(&self) -> std::borrow::Cow<str> {
        (&self.domain).into()
    }
}

#[derive(Serialize, Deserialize)]
struct DomainEntry {
    #[serde(serialize_with = "hex::serde::serialize")]
    #[serde(deserialize_with = "hex::serde::deserialize")]
    pub digest: [u8; 32],
    pub known_domain: Option<String>,
    pub partial_domains: BTreeSet<String>,
}

impl DomainEntry {
    pub fn merge(self, other: Self) -> Self {
        debug_assert_eq!(self.digest, other.digest);

        Self {
            digest: self.digest,
            known_domain: self.known_domain.or(other.known_domain),
            partial_domains: self
                .partial_domains
                .into_iter()
                .chain(other.partial_domains)
                .collect(),
        }
    }
}

impl TryFrom<DomainBlock> for DomainEntry {
    type Error = color_eyre::Report;

    fn try_from(value: DomainBlock) -> std::result::Result<Self, Self::Error> {
        let digest = hex::decode(value.digest)?
            .try_into()
            .map_err(|_| color_eyre::Report::msg("Failed to parse digest"))?;

        // TODO: validate digest?

        let domain_is_known = !value.domain.contains('*');

        Ok(Self {
            digest,
            known_domain: domain_is_known.then(|| value.domain.clone()),
            partial_domains: (!domain_is_known)
                .then(|| {
                    let mut set = BTreeSet::new();
                    set.insert(value.domain);
                    set
                })
                .unwrap_or_default(),
        })
    }
}

impl DatabaseObject for DomainEntry {
    const KEY_NAME: &'static str = "domain";

    fn get_id(&self) -> std::borrow::Cow<str> {
        hex::encode(self.digest).into()
    }
}
