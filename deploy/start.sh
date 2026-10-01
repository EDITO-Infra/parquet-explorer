#!/bin/sh
set -e
nginx
exec parquet-explorer-backend
