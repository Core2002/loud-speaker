mod client;
mod server;

use clap::{arg, command};
use client::client_init;
use cpal::{
    traits::{DeviceTrait, HostTrait},
    Device, Host, SupportedStreamConfig,
};
use loud_speaker::{loud, mic, AudioBus};
use server::server_init;
use std::sync::{Arc, Mutex};

#[tokio::main]
async fn main() {
    let matches = command!()
        .arg(
            arg!(-d --debug <MODE> "运行模式: 0=单机扩音, 1=服务端, 2=客户端")
                .value_parser(clap::value_parser!(u8).range(0..=2))
                .default_value("0"),
        )
        .arg(arg!(-b --bind <ADDR> "服务端监听地址（模式 1）").default_value("0.0.0.0:8080"))
        .arg(arg!(-s --server <URL> "服务端地址（模式 2）").default_value("ws://127.0.0.1:8080"))
        .get_matches();

    let mode = *matches.get_one::<u8>("debug").expect("运行模式");
    let bind_addr = matches.get_one::<String>("bind").expect("监听地址");
    let server_url = matches.get_one::<String>("server").expect("服务端地址");

    let host = cpal::default_host();

    match mode {
        // Same machine: microphone -> speaker.
        0 => {
            let (input_device, input_config) = default_input(&host);
            let (output_device, output_config) = default_output(&host);
            let bus = Arc::new(Mutex::new(AudioBus::new(&input_config, &output_config)));

            mic(bus.clone(), input_device, input_config);
            loud(bus.clone(), output_device, output_config);
            println!("单机扩音已启动：麦克风 → 扬声器（Ctrl+C 退出）");

            if let Err(err) = tokio::signal::ctrl_c().await {
                eprintln!("等待 Ctrl+C 失败: {err}");
            }
        }
        // Capture the microphone and broadcast it over WebSocket.
        1 => {
            let (input_device, input_config) = default_input(&host);
            let sample_rate = input_config.sample_rate().0;
            let bus = Arc::new(Mutex::new(AudioBus::from_rates(
                sample_rate,
                sample_rate,
                1,
            )));

            mic(bus.clone(), input_device, input_config);
            server_init(bus, bind_addr, sample_rate).await;
        }
        // Receive audio from a WebSocket server and play it locally.
        2 => {
            let (output_device, output_config) = default_output(&host);
            let out_rate = output_config.sample_rate().0;
            let out_channels = output_config.channels() as usize;
            // Replaced with the server's sample rate when the hello arrives.
            let bus = Arc::new(Mutex::new(AudioBus::from_rates(
                out_rate,
                out_rate,
                out_channels,
            )));

            loud(bus.clone(), output_device, output_config);
            client_init(bus, server_url, out_rate, out_channels).await;
        }
        _ => eprintln!("未知模式 {mode}：可选 0（单机扩音）、1（服务端）、2（客户端）"),
    }
}

fn default_input(host: &Host) -> (Device, SupportedStreamConfig) {
    let device = host.default_input_device().expect("没有找到默认输入设备");
    let config = device.default_input_config().expect("输入设备配置错误");
    (device, config)
}

fn default_output(host: &Host) -> (Device, SupportedStreamConfig) {
    let device = host.default_output_device().expect("没有找到默认输出设备");
    let config = device.default_output_config().expect("输出设备配置错误");
    (device, config)
}
