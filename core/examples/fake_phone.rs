//! A phone that pairs through the real link protocol, for end-to-end tests.
//!
//! Usage: `cargo run --example fake_phone -- <ticket> [name]`
//!
//! The process prints the six-digit confirmation code, waits for the computer
//! to approve or deny it, and then prints the paired computer id.

use std::error::Error;
use std::io::Write;
use std::sync::{Arc, Mutex};

use elesdee_link::{ComputerRecord, LinkClient, PhoneStore, StoreError};

#[derive(Default)]
struct MemoryStore {
    identity: Mutex<Option<[u8; 32]>>,
    computers: Mutex<Vec<ComputerRecord>>,
}

impl PhoneStore for MemoryStore {
    fn load_identity(&self) -> Result<Option<[u8; 32]>, StoreError> {
        Ok(*lock(&self.identity))
    }

    fn save_identity(&self, secret: &[u8; 32]) -> Result<(), StoreError> {
        *lock(&self.identity) = Some(*secret);
        Ok(())
    }

    fn load_computers(&self) -> Result<Vec<ComputerRecord>, StoreError> {
        Ok(lock(&self.computers).clone())
    }

    fn save_computers(&self, computers: &[ComputerRecord]) -> Result<(), StoreError> {
        *lock(&self.computers) = computers.to_vec();
        Ok(())
    }
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|error| error.into_inner())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let ticket = args.next().ok_or("usage: fake_phone <ticket> [name]")?;
    let name = args.next().unwrap_or_else(|| "E2E Phone".to_owned());

    let store = Arc::new(MemoryStore::default());
    let client = LinkClient::start(store).await?;
    let start = client.pair(&ticket, &name, "e2e").await?;
    let code = start.confirm()?.code.clone();
    println!("CODE {code}");
    std::io::stdout().flush()?;

    match start.finish().await {
        Ok(record) => {
            println!("PAIRED {}", record.id);
            Ok(())
        }
        Err(error) => {
            eprintln!("PAIR FAILED {error}");
            Err(error.into())
        }
    }
}
