#!/bin/bash
set -euo pipefail

VERSION="2021.11"
DIR="buildroot-$VERSION"
ARCHIVE="$DIR.tar.xz"
URL="https://buildroot.org/downloads/$ARCHIVE"
SHA1SUM=861b05ce49d9a5690712b143417228ed220193d9

echo -e "Building Weltempfänger Image\n"

echo "==> Downloading buildroot"
if [ ! -f $ARCHIVE ]; then
    wget $URL
fi

echo "==> Verifying checksum"
if [ $SHA1SUM != "$(sha1sum $ARCHIVE | cut -d' ' -f 1)" ]; then
    echo "ERROR: Invalid checksum"
    exit 1
fi

echo "==> Unpacking buildroot"
tar xf $ARCHIVE

echo "==> Apply config"
rm -f $DIR/.config
cp buildroot-defconfig $DIR/configs/weltempfaenger_defconfig
cd $DIR
cat configs/raspberrypi3_defconfig configs/weltempfaenger_defconfig > configs/merged_defconfig
make merged_defconfig
cd ..

echo "==> Copying board dir"
cp -R --preserve=mode weltempfaenger $DIR/board/
mkdir -p $DIR/board/weltempfaenger/rootfs-overlay/usr/bin/
cp ../inputd/target/arm-unknown-linux-musleabihf/release/inputd $DIR/board/weltempfaenger/rootfs-overlay/usr/bin/

echo "==> Build!"
cd $DIR
make all -j$(($(nproc) - 2))

echo "==> Done! Find the image at $DIR/output/images/sdcard.img"
