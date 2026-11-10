#!/bin/sh
# Fixture commands keep their sh interpreter; per-case bodies are interpreter input.
sf_fixture_source=$0
while [ ! -f "$sf_fixture_source.body" ]; do
    sf_fixture_link=$(readlink "$sf_fixture_source") || exit 1
    case $sf_fixture_link in
        /*) sf_fixture_source=$sf_fixture_link ;;
        *) sf_fixture_source=${sf_fixture_source%/*}/$sf_fixture_link ;;
    esac
done
# Sourcing retains the invoked name and arguments, including an alias used by the caller.
. "$sf_fixture_source.body"
