#!/bin/bash
# Fixture commands share this executable; their freshly written bodies are interpreter input.
symphony_fixture_source=$0
while [ ! -f "$symphony_fixture_source.body" ]; do
    symphony_fixture_link=$(readlink "$symphony_fixture_source") || exit 1
    case $symphony_fixture_link in
        /*) symphony_fixture_source=$symphony_fixture_link ;;
        *) symphony_fixture_source=${symphony_fixture_source%/*}/$symphony_fixture_link ;;
    esac
done
# Sourcing retains the invoked name and arguments, including an alias used by the caller.
. "$symphony_fixture_source.body"
