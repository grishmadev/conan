#!/bin/bash
HOME=$(echo $HOME)
NAME=$(date +%s)
sudo docker run --name $NAME --rm -it \
  --network host -v $(pwd):/app \
  -v $HOME/.cargo/registry:/usr/local/cargo/registry \
  -v $HOME/.cargo/build:/usr/local/cargo/build \
  -v $HOME/.cargo/git:/usr/local/cargo/git \
  -v $HOME/.cargo/.crates/toml:/usr/local/cargo/.crates.toml \
  -v $HOME/.cargo/.global-cache:/usr/local/cargo/.global-cache \
  rust:latest
