// Выпуск: сборка с подписью обновлений → GitHub Release с установщиком и latest.json
// (адрес обновлятора — tauri.conf.json → plugins.updater.endpoints). Подробно — docs/updates.md.
//
//   node scripts/release.mjs --notes "Что нового"    собрать и опубликовать версию из package.json
//   node scripts/release.mjs --dry                   только собрать и подготовить файлы (без публикации)
//
// Ключ подписи — вне репозитория: %USERPROFILE%\.montage-studio\updater.key и .password.

import { execSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, copyFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

const root = join(import.meta.dirname, "..");
const args = process.argv.slice(2);
const dry = args.includes("--dry");
const notesAt = args.indexOf("--notes");
const notes = notesAt >= 0 ? args[notesAt + 1] ?? "" : "";
const REPO = "RimZotik/montage-studio";

const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
const conf = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"));
const cargo = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8").match(/^version = "(.+)"/m)?.[1];
const version = pkg.version;
if (conf.version !== version || cargo !== version) {
  console.error(`версии не совпадают: package.json ${version}, tauri.conf.json ${conf.version}, Cargo.toml ${cargo}`);
  process.exit(1);
}

const keyDir = join(homedir(), ".montage-studio");
const key = readFileSync(join(keyDir, "updater.key"), "utf8");
const password = readFileSync(join(keyDir, "updater.key.password"), "utf8");

console.log(`сборка ${version}…`);
execSync("npm run tauri build", { cwd: root, stdio: "inherit", env: { ...process.env, TAURI_SIGNING_PRIVATE_KEY: key, TAURI_SIGNING_PRIVATE_KEY_PASSWORD: password } });

const nsis = join(root, "src-tauri/target/release/bundle/nsis");
const setup = readdirSync(nsis).find((f) => f.includes(`_${version}_`) && f.endsWith("-setup.exe"));
if (!setup || !existsSync(join(nsis, `${setup}.sig`))) {
  console.error("нет установщика или подписи в", nsis);
  process.exit(1);
}

// Имя файла без пробелов: GitHub заменяет их в именах файлов выпуска.
const asset = `MontageStudio_${version}_x64-setup.exe`;
const out = join(root, "src-tauri/target/release/publish");
mkdirSync(out, { recursive: true });
copyFileSync(join(nsis, setup), join(out, asset));
const latest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    "windows-x86_64": {
      signature: readFileSync(join(nsis, `${setup}.sig`), "utf8").trim(),
      url: `https://github.com/${REPO}/releases/download/v${version}/${asset}`,
    },
  },
};
writeFileSync(join(out, "latest.json"), JSON.stringify(latest, null, 2));
console.log(`готово: ${join(out, asset)}, latest.json`);

if (dry) process.exit(0);
writeFileSync(join(out, "notes.md"), notes || `Montage Studio ${version}`);
execSync(`gh release create v${version} "${join(out, asset)}" "${join(out, "latest.json")}" --repo ${REPO} --title "Montage Studio ${version}" --notes-file "${join(out, "notes.md")}"`, { stdio: "inherit" });
console.log(`опубликовано: https://github.com/${REPO}/releases/tag/v${version}`);
