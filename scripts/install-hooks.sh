#!/bin/sh
# Point git at the repository's hooks folder.
set -eu
git config core.hooksPath scripts/hooks
chmod +x scripts/hooks/*
echo "Git hooks enabled (scripts/hooks)."
