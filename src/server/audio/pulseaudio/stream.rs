/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use std::{io::{ErrorKind, Write}, convert::TryInto};

use audiopus::{coder::Encoder, SampleRate};
use bytes::{Buf, BytesMut};

use pulse::{
    context::{Context},
    sample::Spec,
    stream::Stream,
};

use crate::{server::{audio::pulseaudio::state::PAEvent, device::data::TcpSendHandle}};

use super::EventToAudioServerSender;

#[derive(Debug)]
pub enum PARecordingStreamEvent {
    StateChange,
    Read(usize),
    Moved,
}

#[derive(Debug)]
pub enum PAPlaybackStreamEvent {
    DataOverflow,
    DataUnderflow,
    StateChange,
}

#[derive(Debug)]
pub enum StreamError {
    SocketError(std::io::Error),
    NotEnoughBytesForOneSample,
    EncoderError(audiopus::Error),
}

pub struct PAStreamManager {
    record: Option<(Stream, TcpSendHandle)>,
    sender: EventToAudioServerSender,
    recording_buffer: BytesMut,
    enable_recording: bool,
    quit_requested: bool,
    encoder: Option<Encoder>,
    encoder_buffer: Vec<i16>,
    // Opus documentation recommends size 4000.
    encoder_output_buffer: Box<[u8; 4000]>,
}

impl PAStreamManager {
    pub fn new(sender: EventToAudioServerSender) -> Self {
        Self {
            record: None,
            sender,
            recording_buffer: BytesMut::new(),
            enable_recording: true,
            quit_requested: false,
            encoder: None,
            encoder_buffer: Vec::new(),
            encoder_output_buffer: Box::new([0; 4000]),
        }
    }

    pub fn request_start_record_stream(
        &mut self,
        context: &mut Context,
        source_name: Option<String>,
        send_handle: TcpSendHandle,
        encode_opus: bool,
    ) {
        let rate = if encode_opus {
            self.encoder = Some(Encoder::new(
                SampleRate::Hz48000,
                audiopus::Channels::Stereo,
                audiopus::Application::Audio,
            ).unwrap());
            self.encoder_buffer.clear();
            48000
        } else {
            44100
        };

        let spec = Spec {
            format: pulse::sample::Format::S16le,
            channels: 2,
            rate,
        };

        assert!(spec.is_valid(), "Stream data specification is invalid.");

        let mut stream = Stream::new(context, "Jonect recording stream", &spec, None)
            .expect("Stream creation error");

        stream
            .connect_record(
                source_name.as_deref(),
                None,
                pulse::stream::FlagSet::NOFLAGS,
            )
            .unwrap();

        let mut s = self.sender.clone();
        stream.set_moved_callback(Some(Box::new(move || {
            s.send_pa_record_stream_event(PARecordingStreamEvent::Moved);
        })));

        let mut s = self.sender.clone();
        stream.set_state_callback(Some(Box::new(move || {
            s.send_pa_record_stream_event(PARecordingStreamEvent::StateChange);
        })));

        let mut s = self.sender.clone();
        stream.set_read_callback(Some(Box::new(move |size| {
            s.send_pa_record_stream_event(PARecordingStreamEvent::Read(size));
        })));

        self.recording_buffer.clear();
        self.record = Some((stream, send_handle));
        self.enable_recording = true;
    }

    fn handle_recording_stream_state_change(&mut self) {
        use pulse::stream::State;

        let stream = &self.record.as_ref().unwrap().0;
        let state = stream.get_state();

        match state {
            State::Failed => {
                eprintln!("Recording stream state: Failed.");
            }
            State::Terminated => {
                self.record = None;
                if self.quit_requested {
                    self.sender.send_pa(PAEvent::StreamManagerQuitReady);
                }
                eprintln!("Recording stream state: Terminated.");
            }
            State::Ready => {
                println!("Recording from {:?}", stream.get_device_name());
            }
            _ => (),
        }
    }

    fn handle_data(
        data: &[u8],
        recording_buffer: &mut BytesMut,
        send_handle: &mut TcpSendHandle,
    ) -> Result<(), StreamError> {
        loop {
            if recording_buffer.has_remaining() {
                match send_handle.write(recording_buffer.chunk()) {
                    Ok(count) => {
                        recording_buffer.advance(count);
                    }
                    Err(e) => {
                        if ErrorKind::WouldBlock == e.kind() {
                            break;
                        } else {
                            return Err(StreamError::SocketError(e));
                        }
                    }
                }
            } else {
                break;
            }
        }

        match send_handle.write(data) {
            Ok(count) => {
                if count < data.len() {
                    let remaining_bytes = &data[count..];
                    recording_buffer.extend_from_slice(remaining_bytes);
                    println!(
                        "Recording state: buffering {} bytes.",
                        remaining_bytes.len()
                    );
                }
            }
            Err(e) => {
                if ErrorKind::WouldBlock == e.kind() {
                    recording_buffer.extend_from_slice(data);
                } else {
                    return Err(StreamError::SocketError(e));
                }
            }
        }

        Ok(())
    }

    fn handle_data_with_encoding(
        data: &[u8],
        encoding_buffer: &mut Vec<i16>,
        encoder_output_buffer: &mut [u8; 4000],
        encoder: &mut Encoder,
        recording_buffer: &mut BytesMut,
        send_handle: &mut TcpSendHandle,
    ) -> Result<(), StreamError> {
        if data.len() % 2 != 0 {
            return Err(StreamError::NotEnoughBytesForOneSample);
        }

        let mut sample_iterator = data.chunks_exact(2);

        for bytes in sample_iterator {
            let sample = i16::from_le_bytes(bytes.try_into().unwrap());
            encoding_buffer.push(sample);

            if encoding_buffer.len() == 240 {
                // 240 is 120 samples per channel. That is minimum frame size
                // for Opus at 48 kHz.

                let size = encoder.encode(&encoding_buffer, encoder_output_buffer)
                    .map_err(StreamError::EncoderError)?;

                encoding_buffer.clear();

                Self::handle_data(&encoder_output_buffer[..size], recording_buffer, send_handle)?;
            }
        }

        Ok(())
    }

    fn handle_recording_stream_read(&mut self) {
        let (r, ref mut send_handle) = self.record.as_mut().unwrap();

        if !self.enable_recording {
            return;
        }

        use pulse::stream::PeekResult;

        loop {
            let peek_result = r.peek().unwrap();
            match peek_result {
                PeekResult::Empty => {
                    break;
                }
                PeekResult::Data(data) => {
                    let result = if let Some(encoder) = &mut self.encoder {
                        Self::handle_data_with_encoding(
                            data,
                            &mut self.encoder_buffer,
                            &mut self.encoder_output_buffer,
                            encoder,
                            &mut self.recording_buffer,
                            send_handle
                        )
                    } else {
                        Self::handle_data(data, &mut self.recording_buffer, send_handle)
                    };

                    match result {
                        Ok(()) => (),
                        Err(e) => {
                            eprintln!("Audio streaming error: {:?}", e);
                            r.discard().unwrap();
                            self.enable_recording = false;
                            self.stop_recording();
                            return;
                        }
                    }
                }
                PeekResult::Hole(_) => (),
            }

            r.discard().unwrap();
        }
    }

    pub fn handle_recording_stream_event(&mut self, event: PARecordingStreamEvent) {
        match event {
            PARecordingStreamEvent::StateChange => {
                self.handle_recording_stream_state_change();
            }
            PARecordingStreamEvent::Read(_) => {
                self.handle_recording_stream_read();
            }
            PARecordingStreamEvent::Moved => {
                if let Some(name) = self.record.as_ref().map(|(s, _)| s.get_device_name()) {
                    println!("Recording stream moved. Device name: {:?}", name);
                }
            }
        }
    }

    pub fn stop_recording(&mut self) {
        if let Some((stream, _)) = self.record.as_mut() {
            stream.disconnect().unwrap();
        }
    }

    pub fn request_quit(&mut self) {
        self.quit_requested = true;

        if let Some((stream, _)) = self.record.as_mut() {
            stream.disconnect().unwrap();
        } else {
            self.sender.send_pa(PAEvent::StreamManagerQuitReady);
        }
    }
}
