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

copy:
	echo "Installing to $(install_path)"
	cp -f target/release/$(prog) "$(install_path)"
	cp -f target/release/$(server) "$(install_path)"


install: compile copy
