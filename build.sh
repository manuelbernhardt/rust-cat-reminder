#!/bin/bash

set -o errexit
set -o nounset
set -o xtrace

rsync -a manu@192.168.64.6:~/cat-litter-reminder ~/workspace
sh deploy.sh
