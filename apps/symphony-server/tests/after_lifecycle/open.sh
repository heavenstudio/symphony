#!/bin/bash
# $0 stays the fixture's command path when the shared script is invoked through its symlink.
printf '%s\n' "$*" >> "${0%/*}/../opened"
