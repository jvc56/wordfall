#!/bin/sh
# Refetch the MAGPIE-DATA letter distributions at the pinned commit.
set -e
COMMIT=2a9d65632de20184a1ab76a490c8a27a37fedf78
cd "$(dirname "$0")"
for f in catalan catalan_super dutch dutch_super english english_super french \
         french_super german german_super polish polish_super; do
  curl -sf "https://raw.githubusercontent.com/jvc56/MAGPIE-DATA/$COMMIT/data/letterdistributions/$f.csv" -o "$f.csv"
done
