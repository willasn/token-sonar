// 把 package.json 的 version 同步到 Tauri / Cargo 配置。
// 由 npm 的 version 生命周期自动调用：npm version 0.1.6 / npm version patch
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const abs = (p) => resolve(root, p);
const read = (p) => readFileSync(abs(p), "utf8");

const { version } = JSON.parse(read("package.json"));

const targets = [
  // tauri.conf.json：顶层 version（文件里第一个 "version": 就是它）
  {
    file: "src-tauri/tauri.conf.json",
    pattern: /("version"\s*:\s*")[^"]*(")/,
    replacement: `$1${version}$2`,
  },
  // Cargo.toml：只改 [package] 段的 version（rust-version 不受影响）
  {
    file: "src-tauri/Cargo.toml",
    pattern: /^(version\s*=\s*")[^"]*(")/m,
    replacement: `$1${version}$2`,
  },
];

for (const { file, pattern, replacement } of targets) {
  const before = read(file);
  const after = before.replace(pattern, replacement);
  if (after === before) {
    console.log(`[sync-version] ${file} 已是 ${version}，跳过`);
    continue;
  }
  writeFileSync(abs(file), after);
  console.log(`[sync-version] ${file} → ${version}`);
}

// npm version 只会把 package.json / package-lock.json 加入本次提交，这里补上被改写的配置
try {
  execFileSync("git", ["add", ...targets.map((t) => t.file)], {
    cwd: root,
    stdio: "ignore",
  });
} catch {
  // 非 git 环境或未安装 git 时忽略
}
