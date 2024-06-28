use std::{
    sync::{Arc, Mutex},
    thread,
};

use cpal::{
    traits::{DeviceTrait, StreamTrait},
    Device,
};

pub fn loud(input_device: Device, output_device: Device, buffer: Arc<Mutex<Vec<f32>>>) {
    thread::spawn(move || {
        let input_config = input_device
            .default_input_config()
            .expect("输入设备配置错误");
        let output_config = output_device
            .default_output_config()
            .expect("输出设备配置错误");

        let input_buffer = buffer.clone();
        let input_stream = input_device
            .build_input_stream(
                &input_config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mut buffer = input_buffer.lock().expect("缓冲区锁错误");
                    buffer.extend_from_slice(data);
                },
                move |err| {
                    eprintln!("输入流错误: {:?}", err);
                },
                None,
            )
            .expect("构建输入流时出错");

        let output_stream = output_device
            .build_output_stream(
                &output_config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut buffer = buffer.lock().expect("缓冲区锁错误");
                    let len = data.len().min(buffer.len());
                    data[..len].copy_from_slice(&buffer[..len]);
                    buffer.drain(..len);
                },
                move |err| {
                    eprintln!("输出流错误: {:?}", err);
                },
                None,
            )
            .expect("构建输出流时出错");

        input_stream.play().expect("输入流运行错误");
        output_stream.play().expect("输出流运行错误");

        println!("正在聆听...");
        loop {
            thread::sleep(std::time::Duration::from_secs(60));
        }
    });
}
