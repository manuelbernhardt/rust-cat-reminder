#!/bin/bash

set -o errexit
set -o nounset
set -o xtrace

readonly TARGET_HOST=manu@cat.local
readonly TARGET_HOST_2=manu@cat2.local
readonly TARGET_PATH=/home/manu/cat-litter-reminder
readonly TARGET_ARCH=armv7-unknown-linux-gnueabihf
readonly SOURCE_PATH=./target/${TARGET_ARCH}/release/cat-litter-reminder

cross build --release --target=${TARGET_ARCH}
rsync ${SOURCE_PATH} ${TARGET_HOST}:${TARGET_PATH}
rsync ${SOURCE_PATH} ${TARGET_HOST_2}:${TARGET_PATH}
