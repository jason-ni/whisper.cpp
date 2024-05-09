use ffmpeg_next as ffmpeg;
use ffmpeg::{codec, format, frame, media};
use ffmpeg::software::resampler;
use ffmpeg::ChannelLayout;
use ffmpeg::format::Sample;
use ffmpeg::ffi::AVSampleFormat;
use pretty_hex::*;

use anyhow::{Context, Result};
use crate::errors::WhisperError;
use log::{error, info, debug};
use crate::rb::{Producer, RbProducer};
use std::io::{Write};

pub fn process_audio(audio_file: String, prod: Producer) -> Result<(), WhisperError> {

    let mut ictx = format::input(&audio_file)?;
        //.context("failed to open input audio file")?;

    let i_stream = ictx.streams().best(media::Type::Audio)
        .context("failed to find audio stream")?;

    for (m_k, m_v) in i_stream.metadata().iter() {
        info!("{}: {}", m_k, m_v);
    }

    let audio_stream_idx = i_stream.index();
    // create a decoder for the audio stream
    let context_decoder = codec::context::Context::from_parameters(i_stream.parameters())
        .context("failled to create decoder context")?;

    let mut decoder = context_decoder.decoder().audio()
        .context("audio decoder is required")?;

    // logging info about the audio stream
    info!("audio stream: index: {}, sample_fmt: {:?}, channel_layout: {:?}, rate: {}",
          audio_stream_idx,
          &decoder.format(),
          &decoder.channel_layout(),
          &decoder.rate());

    let sample_fmt = decoder.format();

    let channel_layout = ChannelLayout::default(decoder.channels() as i32);

    let target_channel_layout = ChannelLayout::default(1);
    let mut resampler = resampler(
        (sample_fmt, channel_layout, decoder.rate()),
        (Sample::from(AVSampleFormat::AV_SAMPLE_FMT_S16), target_channel_layout, 16000)
    ).unwrap();

    let mut all_samples_cnt: usize = 0;

    let audio_stream_time_base = i_stream.time_base();
    // iterate packets of the audio stream
    for (stream, mut packet) in ictx.packets() {
        if stream.index() == audio_stream_idx {
            // decode the packet
            packet.rescale_ts(audio_stream_time_base, decoder.time_base());
            decoder.send_packet(&packet).unwrap();
            let mut decoded_frame = frame::Audio::empty();
            while decoder.receive_frame(&mut decoded_frame).is_ok() {
                let timestamp = decoded_frame.timestamp();
                decoded_frame.set_pts(timestamp);
                if decoder.rate() == 16000 {
                    // if the audio stream is already at 16000 Hz, we don't need to resample it
                    let data = decoded_frame.data(0);
                    let fixed_data = bytemuck::cast_slice(&data[..decoded_frame.samples()*2]);
                    prod.write_ext_blocking(fixed_data)?;
                    continue;
                }
                // create a resampler to convert the audio to a different sample rate
                let mut resampled_frame = frame::Audio::empty();
                resampled_frame.set_format(Sample::from(AVSampleFormat::AV_SAMPLE_FMT_S16));
                resampled_frame.set_channel_layout(channel_layout);
                decoded_frame.set_format(decoder.format());
                decoded_frame.set_channel_layout(channel_layout);
                let mut delay_opt = resampler.run(&decoded_frame, &mut resampled_frame).unwrap();
                // copy the resampled data to the decoded_data buffer
                if resampled_frame.samples() > 0 {
                    let data = resampled_frame.data(0);
                    let fixed_data = bytemuck::cast_slice(&data[..resampled_frame.samples()*2]);
                    prod.write_ext_blocking(fixed_data)?;
                }
                all_samples_cnt += resampled_frame.samples();
                while let Some(delay) = delay_opt {
                    delay_opt = resampler.flush(&mut resampled_frame).unwrap();
                    let data = resampled_frame.data(0);
                    let fixed_data = bytemuck::cast_slice(&data[..resampled_frame.samples()*2]);
                    prod.write_ext_blocking(fixed_data)?;
                    all_samples_cnt += resampled_frame.samples();
                }
            }

        }
    }

    //println!("all samples : {:?}", decoded_data);
    println!("all samples cnt: {}", all_samples_cnt);
    prod.close();
    Ok(())
}
