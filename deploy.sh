#!/bin/bash

set -o errexit
set -o nounset
set -o xtrace

readonly TARGET_HOST=manu@192.168.0.226
readonly TARGET_HOST_2=manu@192.168.0.248
readonly TARGET_PATH=/home/manu/cat-litter-reminder
readonly TARGET_ARCH=armv7-unknown-linux-gnueabihf
readonly SOURCE_PATH=./target/${TARGET_ARCH}/release/cat-litter-reminder

cross build --release --target=${TARGET_ARCH}
rsync ${SOURCE_PATH} ${TARGET_HOST}:${TARGET_PATH}
rsync ${SOURCE_PATH} ${TARGET_HOST_2}:${TARGET_PATH}
