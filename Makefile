prog := conan
server := conan-server

install_path := $(CARGO_HOME)/bin
config_path := $(HOME)/.config/conan/
ifndef CARGO_HOME
	install_path := $(HOME)/.local/share/cargo/bin
endif


default: install

compile:
	export "CARGO_PROFILE_RELEASE_LTO=off" && cargo build --release

dev:
	cargo build

copy:
	echo "Installing to $(install_path)"
	cp -f target/release/$(prog) "$(install_path)"
	cp -f target/release/$(server) "$(install_path)"

check:
	 echo "Startng to check code.."
	 echo "Formatting.."
	 cargo fmt
	 echo "Checking for lint errors.."
	 cargo clippy --all-targets --all-features -- -D warnings
	 echo "Testing.."
	 cargo test

install: compile copy
installfull: check install
devfull: check dev
