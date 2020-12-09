Http Pipe
=========

[![Crates.io](https://img.shields.io/crates/v/http-pipe)](https://crates.io/crates/http-pipe)

Piping data from one host to another using a relay server

Features
--------

* Multithreaded transmission
* Automatically recover from network failures

Usage
-----

### Server

Use docker:

```shell
docker run -p 80:8080 cww0614/http-pipe
```

### Client

```shell
# Can also use docker:
# alias http-pipe="docker run -it --rm cww0614/http-pipe"

# Sending
echo 123 | http-pipe http://example.com/endpoint

# Receiving
http-pipe http://example.com/endpoint > output.txt
```
