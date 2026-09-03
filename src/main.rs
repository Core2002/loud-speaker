mod client;
mod server;
use clap::{arg, command};
use client::client_init;
use cpal::traits::{DeviceTrait, HostTrait};
use loud_speaker::{loud, mic, AudioBus};
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

    let input_config = default_input_device
        .default_input_config()
        .expect("输入设备配置错误");
    let output_config = default_output_device
        .default_output_config()
        .expect("输出设备配置错误");

    let bus = Arc::new(Mutex::new(AudioBus::new(&input_config, &output_config)));

    match matches
        .get_one::<u8>("debug")
        .expect("Counts are defaulted")
    {
        0 => {
            println!("Debug mode is off");
            mic(bus.clone(), default_input_device, input_config);
            loud(bus.clone(), default_output_device, output_config);
        }
        1 => {
            println!("server mode is on");
            mic(bus.clone(), default_input_device, input_config);
            server_init().await;
        }
        2 => {
            println!("client mode is on");
            loud(bus.clone(), default_output_device, output_config);
            client_init().await;
        }
        _ => println!("Don't be crazy"),
    }

    loop {
        thread::sleep(std::time::Duration::from_secs(60));
    }
}
