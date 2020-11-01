# Weltempfänger

A raspberry pi based radio in a 50s Grundig radio enclosure.

## Installing

Build plugin:

    cd inputd
    ./build-rpi.sh

Copy plugin to volumio:

    scp target/arm-unknown-linux-musleabihf/release/inputd volumio@volumio:~/

Copy service to volumio and enable it:

    cd ..
    scp inputd.service volumio@volumio:~/
    ssh volumio@volumio
    sudo mv inputd.service /etc/systemd/system/
    sudo systemctl start inputd
    sudo systemctl enable inputd
