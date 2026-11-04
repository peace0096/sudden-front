// Downloads every pack in sources.json into art-src/<id>/ at the repository root, checks each
// download against its recorded SHA-256 and unpacks archives. Everything it writes is data: no file
// from a pack is ever run or imported. Running it again skips what is already in place.
//
// Usage: node fetch.mjs [--record] [pack-id ...]
//   --record  writes the SHA-256 of a download that has no hash yet into sources.json. Use it
//             only for a newly added source, and read its license before relying on the hash.
//
// Layout of art-src/<id>/:
//   download/<archive>  the archive as downloaded
//   content/...         the archive's files, or the pack's single-file downloads
//   fetched.json        which archive hash content/ was unpacked from
// A download whose hash differs from the pin takes what it would have replaced with it: the stale
// file, and for an archive its content/ and fetched.json, so nothing is left to build from.
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { unzipSync } from "fflate";
import { checkPackPaths, inside } from "./lib/paths.mjs";

const here = fileURLToPath(new URL(".", import.meta.url));
const sourcesPath = join(here, "sources.json");
const artSrc = join(here, "..", "..", "art-src");
/** No source is anywhere near this; a larger answer is refused rather than written. */
const MAX_DOWNLOAD = 256 * 1024 * 1024;
const MAX_UNPACKED = 1024 * 1024 * 1024;
const MAX_ENTRIES = 20_000;

const args = process.argv.slice(2);
const record = args.includes("--record");
const wanted = args.filter((arg) => !arg.startsWith("--"));
const sources = JSON.parse(readFileSync(sourcesPath, "utf8"));
try {
  // Before anything is written: every path below is made from these names.
  for (const pack of sources.packs) checkPackPaths(pack);
} catch (error) {
  console.error(`fetch: sources.json: ${error.message}`);
  process.exit(1);
}
const unknown = wanted.filter((id) => !sources.packs.some((pack) => pack.id === id));
if (unknown.length) {
  console.error(`fetch: no pack ${unknown.join(", ")} in sources.json`);
  process.exit(1);
}

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const kb = (bytes) => `${(bytes / 1024).toFixed(0)} KB`;
let recorded = 0;
const problems = [];

async function download(url) {
  if (!url.startsWith("https://")) throw new Error(`refusing a non-https URL: ${url}`);
  const response = await fetch(url, { redirect: "follow", headers: { "user-agent": "sudden-front-art-tools" } });
  if (!response.ok) throw new Error(`HTTP ${response.status} for ${url}`);
  const declared = Number(response.headers.get("content-length") ?? 0);
  if (declared > MAX_DOWNLOAD) throw new Error(`${url} is ${declared} bytes, over the ${MAX_DOWNLOAD} limit`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  if (bytes.length > MAX_DOWNLOAD) throw new Error(`${url} is over the ${MAX_DOWNLOAD} byte limit`);
  return bytes;
}

/** A download that is not the file sources.json pins. */
class PinMismatch extends Error {}

/** The recorded hash matches, or there was none and --record stored this one. */
function verify(entry, bytes, label) {
  const actual = sha256(bytes);
  if (!entry.sha256) {
    if (!record) throw new Error(`${label} has no sha256 in sources.json; rerun with --record to pin ${actual}`);
    entry.sha256 = actual;
    recorded++;
    console.log(`  recorded ${label} sha256 ${actual}`);
    return;
  }
  if (actual !== entry.sha256) throw new PinMismatch(`${label} sha256 is ${actual}, sources.json expects ${entry.sha256}`);
}

/**
 * Downloads url to path unless a file with the expected hash is already there. A file there that
 * doesn't have it is stale, and goes if the download doesn't match the pin either.
 */
async function fetchFile(entry, url, path, label, check) {
  if (existsSync(path) && entry.sha256 && sha256(readFileSync(path)) === entry.sha256) return false;
  const bytes = await download(url);
  try {
    verify(entry, bytes, label);
  } catch (error) {
    if (error instanceof PinMismatch) rmSync(path, { force: true });
    throw error;
  }
  check?.(bytes);
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(`${path}.part`, bytes, { mode: 0o644 });
  renameSync(`${path}.part`, path);
  return true;
}

function isGlb(bytes) {
  if (String.fromCharCode(...bytes.subarray(0, 4)) !== "glTF") throw new Error("not a GLB file");
}

/** Where an archive entry goes, or null for an entry that must not be written. */
function entryPath(root, name) {
  const parts = name.replace(/\\/g, "/").split("/");
  if (name.endsWith("/") || parts[0] === "__MACOSX" || parts.at(-1) === ".DS_Store") return null;
  if (/^[A-Za-z]:$/.test(parts[0]) || parts.some((part) => part === "" || part === "." || part === "..")) {
    throw new Error(`unsafe path in archive: ${name}`);
  }
  const path = join(root, ...parts);
  if (!path.startsWith(root + sep)) throw new Error(`unsafe path in archive: ${name}`);
  return path;
}

function unpack(bytes, root) {
  let total = 0;
  let entries = 0;
  const files = unzipSync(bytes, {
    filter(file) {
      if (++entries > MAX_ENTRIES) throw new Error(`more than ${MAX_ENTRIES} entries`);
      total += file.originalSize;
      if (total > MAX_UNPACKED) throw new Error(`unpacks to more than ${MAX_UNPACKED} bytes`);
      return entryPath(root, file.name) !== null;
    },
  });
  rmSync(root, { recursive: true, force: true });
  let written = 0;
  for (const [name, data] of Object.entries(files)) {
    const path = entryPath(root, name);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, data, { mode: 0o644 });
    written++;
  }
  return { files: written, bytes: total };
}

/** License-looking files in a pack, so they can be read before anything is used. */
function licenseFiles(root) {
  if (!existsSync(root)) return [];
  const found = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (/licen[cs]e|readme|copying/i.test(entry.name)) found.push(path.slice(root.length + 1));
    }
  };
  walk(root);
  return found;
}

for (const pack of sources.packs) {
  if (wanted.length && !wanted.includes(pack.id)) continue;
  const dir = inside(artSrc, pack.id, "pack id");
  const content = join(dir, "content");
  mkdirSync(dir, { recursive: true });
  try {
    if (pack.files) {
      let fetched = 0;
      for (const file of pack.files) {
        if (await fetchFile(file, file.download, inside(content, file.name, `pack ${pack.id} file`), `${pack.id}/${file.name}`, isGlb)) fetched++;
      }
      console.log(`${pack.id}: ${pack.files.length} files (${fetched} downloaded)`);
    } else {
      const archive = inside(join(dir, "download"), pack.archive, `pack ${pack.id} archive`);
      const marker = join(dir, "fetched.json");
      let downloaded;
      try {
        downloaded = await fetchFile(pack, pack.download, archive, `${pack.id}/${pack.archive}`);
      } catch (error) {
        // The pinned archive can't be had, so neither can content/ be shown to come from it: it
        // goes with its marker, and build-art.mjs refuses the pack until a fetch succeeds.
        if (error instanceof PinMismatch) {
          rmSync(content, { recursive: true, force: true });
          rmSync(marker, { force: true });
        }
        throw error;
      }
      const done = existsSync(marker) && JSON.parse(readFileSync(marker, "utf8")).sha256 === pack.sha256 && existsSync(content);
      if (!done) {
        const result = unpack(readFileSync(archive), content);
        writeFileSync(marker, JSON.stringify({ archive: pack.archive, sha256: pack.sha256, ...result }, null, 2) + "\n");
      }
      const size = statSync(archive).size;
      console.log(`${pack.id}: ${pack.archive} ${kb(size)}${downloaded ? " downloaded" : ""}${done ? "" : ", unpacked"}`);
      const licenses = licenseFiles(content);
      if (licenses.length) console.log(`  license/readme files: ${licenses.join(", ")}`);
    }
  } catch (error) {
    problems.push(`${pack.id}: ${error.message}`);
    console.error(`${pack.id}: FAILED ${error.message}`);
  }
}

if (recorded) {
  writeFileSync(sourcesPath, JSON.stringify(sources, null, 2) + "\n");
  console.log(`recorded ${recorded} new hash(es) in sources.json`);
}
if (problems.length) {
  console.error(`fetch: ${problems.length} pack(s) failed`);
  process.exit(1);
}
