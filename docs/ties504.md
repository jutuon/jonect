# Some required information for TIES504 university course

## Why the program is usefull?

Check section "Application Description and Mission" from
[architechture_plan.md](./architechture_plan.md#application-description-and-mission)
file.

## Program's environment

Rust app works on Ubuntu 20.04 and Android app works on Android 5.0 or later.

End users should use the apps in home or otherwise private networks. Server TCP
ports 8080 and 8082 are connectable from the networks if there is no firewall
used. All data is unencrypted.

Network is not required as USB connection will also work.

## Requirements

Requirements in the
[architechture_plan.md](./architechture_plan.md#prototype-requirements) are not
completely implemented. From that file only the following functional
requirements are implemented:

* Computer speaker functionality
* USB cable connection
* WLAN connection

Audio compression was also an requirement for the app but I didn't wrote that to
the plan file for some reason (probably I did not tought it as a high level
requirement). That is implemented using Opus codec.

## Features

Rust app's features:

- Settings file which is not currently used for anything usefull.
- GUI mode for controlling the server. It does not implement any usefull
  features yet.
- Test client mode.
- Record PCM audio from PulseAudio.
- Send PCM audio to the connected client using TCP.

Android app's features:

- Connect to the server.
- Play PCM or Opus audio.
- Option to connect automatically to the server.
- Store previously used server IP address.

Somewhat better descriptions for some features can be found from main readme
files located in repositories root directories for both apps. (Android app is in
a different repository.)

## What was hard?

Async Rust was new thing to learn. I had to basically rewrite the server
program before the program structure was good enough.

Creating new Glib context object for different thread than GTK main thread. I
got that working but I ended up creating TCP connection to the UI to make sure
that everything Glib related works ok. Before that the GTK and PulseAudio code was
running in the same process.

Using Java's ByteBuffer object. I remember that I forgot to rewind the buffer at
least once so buffer code required a bit debugging.

Adding Opus decoding to the Android app. I tought that I use Android's built in
support for that but it's documentation was a bit incomplete or unclear, so I
ended up using JNI and libopus with Rust.

Finding decoding bug. To make debugging easier I ended up changing JNI
ByteBuffer handling to a TCP socket. The bug was in the Rust code.

## What was easy?

Non async Rust coding as I have used it a lot before.

Adding Rust code to Android app was suprisingly easy.

## What I should have done differently?

Better planing and using Rust code in the Android app from the start would
probably have saved some time. However better planing of server's structure was
basically impossible because I had not experience with async Rust when I started
this project.

Also maybe UDP for audio data would have been better than TCP. This was inferior
design by on purpose as I prioritized other stuff like server coding and looked
forward to writing my master thesis on improving app's audio latency.

## Course credits and grade

I have used enough time for 8 credits. I think that grade 3 would be good
because I couldn't manage to finish the project on schedule at least twice,
documentation was mostly done quickly and there are some known issues in both of
the apps.
