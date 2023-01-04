#!/bin/bash

# Deletes a bunch of cache entries in Github Actions. We only need the latest
# cache entry, which will be written after the Action run.

RESPONSE=$(curl -i \
  -H "Accept: application/vnd.github+json" \
  -H "Authorization: Bearer $GITHUB_TOKEN"  \
  https://api.github.com/repos/klausi/mts-cron/actions/caches )
echo "$RESPONSE"
CACHE_IDS=( $(echo "$RESPONSE" | grep '"id"' | grep -o '[0-9]*') )

for CACHE_ID in "${CACHE_IDS[@]}"
do
  echo "Deleting cache ID $CACHE_ID"
  curl \
  -X DELETE \
  -H "Accept: application/vnd.github+json" \
  -H "Authorization: Bearer $GITHUB_TOKEN" \
  "https://api.github.com/repos/klausi/mts-cron/actions/caches/$CACHE_ID"
done
