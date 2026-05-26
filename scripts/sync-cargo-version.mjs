#!/usr/bin/env node
// sync-cargo-version.mjs
//
// `changeset version` bumps package.json + writes CHANGELOG.md but it
// doesn't know about Cargo.toml. This script reads the version we just
// landed in package.json and mirrors it into Cargo.toml's
// [workspace.package] block, then re-resolves Cargo.lock so the lock
// reflects the new workspace version.
//
// Runs as the `postversion` step from `pnpm version`. Idempotent.

import { execSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';

const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
const next = pkg.version;
if (!/^\d+\.\d+\.\d+/.test(next)) {
    console.error(`refusing to sync — package.json version doesn't look like semver: ${next}`);
    process.exit(1);
}

const cargoPath = 'Cargo.toml';
const cargo = readFileSync(cargoPath, 'utf8');

// Only touch the workspace.package version line. Match anchored to the
// first occurrence inside the [workspace.package] section.
const updated = cargo.replace(
    /(\[workspace\.package][\s\S]*?\nversion\s*=\s*")[^"]+(")/,
    `$1${next}$2`,
);

if (updated === cargo) {
    console.error("couldn't find [workspace.package] version line in Cargo.toml");
    process.exit(1);
}

writeFileSync(cargoPath, updated);
console.log(`sync-cargo-version: ${cargoPath} bumped to ${next}`);

// Refresh Cargo.lock so the new version propagates. `-p smooblue-app`
// (the binary crate) is enough — Cargo updates everything in the
// workspace that references it.
try {
    execSync('cargo update --workspace --offline 2>/dev/null || cargo update --workspace', {
        stdio: 'inherit',
    });
    console.log('sync-cargo-version: Cargo.lock refreshed');
} catch (e) {
    console.warn('sync-cargo-version: cargo update failed (continuing); run it manually before pushing');
}
