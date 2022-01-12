# Jonect

Jonect server, test client and GUI for controlling the server.

This app works on Ubuntu 20.04.

## Project status

Server and Jonect Android client work as a speaker app. Server records audio
from PulseAudio and sends it to the client. PCM and Opus audio streams are
supported.

Test client connects to the server and displays audio stream data speed.

GUI does not have any usefull features.

## Building and running on Ubuntu 20.04

1. Install some packages.

    ```
    sudo apt install libgtk-3-dev build-essential libopus-dev
    ```

2. Install Rust.

    <https://www.rust-lang.org/>

3. Build and run the server. Default recording output will be recorded when
   audio streaming starts.

    ```
    cargo run --release
    ```

## User guide

Server binary includes the test client and GUI modes as well.

### Server

Server mode starts with `cargo run --release` command.

Server uses TCP port 8080 for client data connections and TCP port 8082 for
audio data. Connections over network interfaces are possible (socket is binded
using `0.0.0.0` address).

Server uses TCP port 8081 for GUI connection. Only localhost connections are
possible.

1. Decide what audio stream the server will send to the client. By default the
   default recording output will be recorded when audio streaming starts. Or
   possibly the previously selected audio stream if PulseAudio Volume Control
   GUI program is used to select the audio stream.

   Opionally select PulseAudio monitor stream to record when audio streaming
   starts. This can be done selecting PulseAudio monitor source with command
   line argument. Server prints available monitor sources when the server
   starts. Copy name printed after `monitor_source` text and start the server
   with command `cargo run --release -- -s MONITOR_SOURCE_NAME`.

   Monitor streams are recordable streams of output device audio. Depending on
   the device monitor stream volume is controlled by device output volume
   controls.

   Other possiblility is use PulseAudio Volume Control GUI program which can be
   installed using command `sudo apt install pavucontrol`. With this program it
   is possible to change recording stream/device on the fly.

   It is also possible to create new virtual output device for example if normal
   audio output from speakers is not wanted.
   <https://unix.stackexchange.com/questions/174379/how-can-i-create-a-virtual-output-in-pulseaudio>

2. Decide should the server encode audio with Opus codec. Running the server
   with command `cargo run --release -- -e` will enable the Opus codec.

3. Start the server and connect the client to it.

### Test client

Test client can be started for example using this command:
`cargo run --release -- client -a localhost:8080`

### GUI

GUI can be started for example using this command:
`cargo run --release -- gui`

1. Start the server.

2. Start the GUI.

3. Press Test button. Server should print text "UI notification" to the console.

## Known issues

If client is disconnected and reconnected too quickly it is possible that audio
stream does not start.

## License

Project license is MPL 2.0.
