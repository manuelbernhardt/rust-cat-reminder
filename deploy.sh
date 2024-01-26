#!/bin/bash

set -o errexit
set -o nounset
set -o xtrace

allNodes=("cat" "cat1" "cat2" "cat3" "cat4")

readonly TARGET_HOST=manu@cat1.local
readonly TARGET_HOST_2=manu@cat2.local
readonly TARGET_PATH=/home/manu/cat-litter-reminder
readonly TARGET_ARCH=armv7-unknown-linux-gnueabihf
readonly SOURCE_PATH=./target/${TARGET_ARCH}/release/cat-litter-reminder

cross build --release --target=${TARGET_ARCH}
for n in ${allNodes[@]}; do
  rsync ${SOURCE_PATH} manu@${n}.local:${TARGET_PATH}
done
