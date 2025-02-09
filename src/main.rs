mod client;
mod server;
use clap::{arg, command};
use client::client_init;
use cpal::traits::HostTrait;
use loud_speaker::{loud, mic};
use server::server_init;
use std::{
    sync::{Arc, Mutex},
    thread,
};

#[tokio::main]
async fn main() {
    let matches = command!()
        .arg(arg!(
            -d --debug ... "Turn debugging information on"
        ))
        .get_matches();

    let host = cpal::default_host();
    let default_input_device = host.default_input_device().expect("没有找到默认输入设备");
    let default_output_device = host.default_output_device().expect("没有找到默认输出设备");

    let buffer = Arc::new(Mutex::new(Vec::new()));
    let bfc = buffer.clone();

    match matches
        .get_one::<u8>("debug")
        .expect("Counts are defaulted")
    {
        0 => {
            println!("Debug mode is off");
            mic(default_input_device, buffer.clone());
            loud(default_output_device, buffer.clone());
        }
        1 => {
            println!("server mode is on");
            mic(default_input_device, buffer.clone());
            server_init().await;
        }
        2 => {
            println!("client mode is on");
            loud(default_output_device, buffer.clone());
            client_init(bfc).await;
        }
        _ => println!("Don't be crazy"),
    }

    loop {
        thread::sleep(std::time::Duration::from_secs(60));
    }
    // Ok(())
}
