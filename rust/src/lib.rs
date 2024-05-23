mod accel;
mod audio;
mod errors;
mod rb;

use std::io::Write;
use rb::{Producer, Consumer, SpscRb};
use crate::audio::process_audio;
use crate::rb::{RB, RbConsumer, SampleRange};

#[cxx::bridge(namespace = "WhisperRust")]
mod ffi {

    extern "Rust" {

        type SenderWrapper;

        fn send_text(sender: &SenderWrapper, text: String);

        fn run_transcript(audio_file: String);
    }

    unsafe extern "C++" {
        include!("whisper_wrapper.h");

        type WhisperWrapper;

        pub unsafe fn infer_buffer(&self, sender: &SenderWrapper, buffer: *const f32, buffer_size: usize) -> i32;
        pub unsafe fn get_segment_count(&self) -> i32;
        pub unsafe fn create_whisper_wrapper(model_path: &str) -> UniquePtr<WhisperWrapper>;
    }
}

pub struct SenderWrapper {
    sender: std::sync::mpsc::SyncSender<String>,
}

impl SenderWrapper {
    pub fn new(sender: std::sync::mpsc::SyncSender<String>) -> Self {
        Self { sender }
    }
}

pub fn send_text(sender: &SenderWrapper, text: String) {
    sender.sender.send(text).unwrap();
}


pub fn run_transcript(audio_file: String) {
    let logger_name = "rust_wrapper";
    env_logger::builder()
        .format(move |buf, record| {
            writeln!(buf, "[{}][{}]<{}> - {}",
                     &logger_name,
                     chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                     record.level(), record.args())
        })
        .init();
    println!("Hello from Rust!");

    let rb_obj = SpscRb::new(16000*120);
    let prod = rb_obj.producer();
    let cons = rb_obj.consumer();

    let (text_tx, text_rx) = std::sync::mpsc::sync_channel(10);

    let t1 = std::thread::spawn(move || {
        match process_audio(audio_file, prod) {
            Ok(_) => log::info!("Audio processed successfully!"),
            Err(e) => log::error!("Error processing audio: {}", e.to_string()),

        };
    });

    let t2 = std::thread::spawn(move || {
        let ww = unsafe {
            ffi::create_whisper_wrapper(
                "/media/msd/models/ggml-large-v3-q5_0.bin")
        };
        let mut bufferf32: Vec<f32> = vec![0.0; 16000*120];
        let mut global_pos = 0usize;
        let VAD_FRAME_SIZE = 16000;
        let sender_wrapper = SenderWrapper::new(text_tx);
        loop {
            match cons.peek_blocking(global_pos, &mut bufferf32[..VAD_FRAME_SIZE*3]) {

                Ok(sample_range) => {
                    match sample_range {
                        SampleRange::Adjacent(buf, buf_size) => {
                            global_pos += buf_size;
                            log::info!("Received {} samples", buf_size);
                            //(unsafe{ offline_stream_engine.vad_infer_buffer(buf, buf_size, false)}, false )
                            let ret = unsafe{ww.infer_buffer(&sender_wrapper, buf, buf_size)};
                            log::info!("Processed {} samples: ret: {}", buf_size, ret);
                            ()
                        }
                        SampleRange::NonAdjacent(buf_size) => {
                            global_pos += buf_size;
                            //(unsafe{ offline_stream_engine.vad_infer_buffer(bufferf32[..buf_size].as_ptr(), buf_size, false)}, false)
                            log::info!("Received {} samples", buf_size)
                        }
                        SampleRange::EofEmpty => panic!("Unexpected EOF"),
                    }
                },
                Err(rb::RbError::EOF(sample_range)) => {
                    match sample_range {
                        SampleRange::Adjacent(buf, buf_size) => {
                            global_pos += buf_size;
                            if buf_size % 800 != 0 {
                                //(unsafe{ offline_stream_engine.vad_infer_buffer(buf, buf_size - (buf_size % 800), true)}, true)
                                log::info!("Received {} samples", buf_size)
                            } else {
                                //(unsafe{ offline_stream_engine.vad_infer_buffer(buf, buf_size, true)}, true)
                                log::info!("Received {} samples", buf_size)
                            }
                        }
                        SampleRange::NonAdjacent(buf_size) => {
                            global_pos += buf_size;
                            //(unsafe{ offline_stream_engine.vad_infer_buffer(bufferf32[..buf_size].as_ptr(), buf_size, true)}, true)
                            log::info!("Received {} samples", buf_size)
                        }
                        //SampleRange::EofEmpty=> (vec![], true),
                        SampleRange::EofEmpty=> (),
                    }
                }
                _ => panic!("Unexpected rb error"),
            }
        }
    });



    for text in text_rx {
        log::info!("Received text: {}", text);
    }
    t1.join().unwrap();
    t2.join().unwrap();
}