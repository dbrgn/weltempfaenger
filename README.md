# Weltempfänger

A raspberry pi based radio in a 50s Grundig radio enclosure.

## Installing

On Arch Linux, install `muslcc-arm-linux-musleabihf-cross-bin` from AUR.

Build plugin:

    cd inputd
    ./build-rpi.sh

Copy plugin to volumio:

    scp target/arm-unknown-linux-musleabihf/release/inputd volumio@volumio:~/

Fix GPIO group permissions. In `/etc/udev/rules.d/92-gpio.rules`, add `GROUP="gpio"`:

```diff
-SUBSYSTEM=="gpio", KERNEL=="gpiochip*", ACTION=="add", PROGRAM="/bin/sh -c 'chown volumio:volumio /sys/class/gpio/export /sys/class/gpio/unexport ; chmod 220 /sys/class/gpio/export /sys/class/gpio/unexport'"
-SUBSYSTEM=="gpio", KERNEL=="gpio*", ACTION=="add", PROGRAM="/bin/sh -c 'chown volumio:volumio /sys%p/active_low /sys%p/direction /sys%p/edge /sys%p/value ; chmod 660 /sys%p/active_low /sys%p/direction /sys%p/edge /sys%p/value'"
+SUBSYSTEM=="gpio", KERNEL=="gpiochip*", GROUP="gpio", ACTION=="add", PROGRAM="/bin/sh -c 'chown volumio:volumio /sys/class/gpio/export /sys/class/gpio/unexport ; chmod 220 /sys/class/gpio/export /sys/class/gpio/unexport'"
+SUBSYSTEM=="gpio", KERNEL=="gpio*", GROUP="gpio", ACTION=="add", PROGRAM="/bin/sh -c 'chown volumio:volumio /sys%p/active_low /sys%p/direction /sys%p/edge /sys%p/value ; chmod 660 /sys%p/active_low /sys%p/direction /sys%p/edge /sys%p/value'"
```

(Details: https://github.com/volumio/Build/issues/424)

Copy service to volumio and enable it:

    cd ..
    scp inputd.service volumio@volumio:~/
    ssh volumio@volumio
    sudo mv inputd.service /etc/systemd/system/
    sudo systemctl start inputd
    sudo systemctl enable inputd
