# About app's architechture

Last update: 2022-01-12

## High level architechture

![High level architechture](./high_level_architechture.svg)

## Server

### Internal structure

![Server's internal structure](./server_structure.svg)

### Component communication

Components which are Tokio tasks or normal threads use message passing to
communicate.

![Server component communication](./component_communication.svg)

### Data storage

Rust app (server, test client and GUI) creates TOML settings file. But it is not used
for anything usefull so there is no diagram about it.
