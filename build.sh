#!/bin/bash

set -o errexit
set -o nounset
set -o xtrace

rsync -a manu@192.168.64.3:~/cat-litter-reminder ~/workspace/cat-litter-reminder
sh deploy.sh
