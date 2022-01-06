# Weltempfänger

A Raspberry Pi based radio in a 50s Grundig radio enclosure.

The input daemon reads from GPIO pins (wired to the buttons) and from an ADC
(wired to the volume potentiometer).

The Linux/Systemd based system image for the Raspberry Pi is built with
Buildroot.

Schematics for the wiring are provided as a LibrePCB project.

## 1. Building inputd

On Arch Linux, install `muslcc-arm-linux-musleabihf-cross-bin` from AUR.

Build plugin:

    cd inputd
    ./build-rpi.sh

## 2. Build custom image for Raspberry Pi Zero W

Build image with buildroot:

    cd rpi-image/
    ./build.sh
