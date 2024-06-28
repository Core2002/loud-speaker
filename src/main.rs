mod client;
mod server;
use cpal::traits::HostTrait;
use loud_speaker::loud;
use server::server_init;
use std::{sync::{Arc, Mutex}, thread};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let host = cpal::default_host();
    let buffer = Arc::new(Mutex::new(Vec::new()));
    loud(
        host.default_input_device().expect("没有找到默认输入设备"),
        host.default_output_device().expect("没有找到默认输出设备"),
        buffer.clone(),
    );

    // server_init();
    loop {
        thread::sleep(std::time::Duration::from_secs(60));
    }
    // Ok(())
}
