# Architecture of the Jonect Android Application

This is old plan for the project and does not complately represent the current
(2022-01-13) state or future of the project. It is mostly written before there
was any coding done.

## Application Description and Mission

Jonect is an all-in-one smartphone application and computer server software
solution which allows your phone to operate as computer speaker,
microphone, webcam or something else which can be implemented just using
your smartphone.

But why to design and develop this kind of software?

* Environmental impact. Smartphones don't get as software updates as long as
normal computers. Old smartphones with old software with security
vulnerabilities can be repurposed as computer peripherals.
Also, Jonect will reduce the need to buy some computer peripherals.
* Convenience. If you already own a smartphone, but not a computer
microphone why you should have to buy a specific device for that as your
phone includes a microphone?

## Application Prototype

Create minimum viable product which allows your smartphone to operate as an
computer speaker.

## Prototype requirements

### Functional

* Chromecast like simplicity
* Computer speaker functionality
* USB cable connection
* WLAN connection

### Non-functional

* Sound latency should be unnoticeable compared to a normal computer
speaker and sound card setup at least using USB cable connection.
* WLAN connection should have option to encrypt application data to make
data capturing harder.

## Prototype Architecture

Jonect uses layers to implement all possible data transfer options. Android
platform restricts application and computer communication possibilities to TCP
and UDP sockets. The prototype will use TCP to simplify the development effort.
(Correction 2022-01-12: USB data connection with Android USB accessory mode
should be also possible.)

* Application data layer
* Encryption layer: TLS or no encryption
* Protocol layer: TCP
* Connection layer: USB or WLAN
