// 配布用のフロントエンド(dist)に、E2Eテスト専用のコードが混入していないことを確かめる。
//
// E2Eビルド(VITE_E2E_TESTING=true)だけが持つ次の3つは、配布物に含まれると、画面の外から
// ファイルダイアログの結果を指定したり、インポートの保留の識別子を読んだり、認証無しのWebDriver経由で
// アプリを操作したりする入口になる。
// - src/lib/file-dialog.tsの差し替え口(window.__e2eFileDialogPaths)
// - src/lib/e2e-pending-import.tsの、受け取った保留の識別子の記録口(window.__e2ePendingImportIds)
// - src/main.tsxが読み込むWebDriverプラグイン(@wdio/tauri-plugin)
// どちらも、VITE_E2E_TESTINGが"true"のときだけ残る定数分岐の中にあり、通常のビルドでは消える。
// その分岐が壊れていないことを、ビルドした結果のファイルの中身で確かめる。
//
// 使い方: bun run scripts/check-dist.ts [distのパス(省略時はgui/dist)]

import { readdirSync, readFileSync, statSync } from "node:fs";
import { dirname, extname, join, relative, resolve } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

// 大文字小文字を区別せずに探す。VITE_E2E_TESTINGは、環境変数の名前そのものが残っていないことの確認
// (定数分岐が畳まれず、環境の値をまとめて埋め込む書き方に変わっていないか)。
const FORBIDDEN_MARKERS = ["__e2e", "wdio", "VITE_E2E_TESTING"];

const TEXT_EXTENSIONS = new Set([".js", ".mjs", ".cjs", ".css", ".html", ".json", ".map", ".svg"]);

function listFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? listFiles(path) : [path];
  });
}

const scriptDir = dirname(fileURLToPath(import.meta.url));
const distDir = resolve(process.argv[2] ?? join(scriptDir, "..", "dist"));

let files: string[];
try {
  files = listFiles(distDir);
} catch {
  console.error(`distが見つからない: ${distDir}(先にフロントエンドをビルドする)`);
  process.exit(2);
}

// 空のdistや別のフォルダを指したままでは、何も見つからないだけで通ってしまうため、通さない。
if (!files.some((file) => extname(file) === ".js") || !files.some((file) => file.endsWith("index.html"))) {
  console.error(`distにindex.htmlまたはJavaScriptが無い: ${distDir}(ビルドの結果ではない可能性がある)`);
  process.exit(2);
}

const findings: string[] = [];
let scanned = 0;
for (const file of files) {
  if (!TEXT_EXTENSIONS.has(extname(file))) continue;
  scanned += 1;
  const content = readFileSync(file, "utf-8").toLowerCase();
  for (const marker of FORBIDDEN_MARKERS) {
    if (content.includes(marker.toLowerCase())) {
      findings.push(`${relative(distDir, file)}: ${marker}`);
    }
  }
}

if (findings.length > 0) {
  console.error("配布用のフロントエンドに、E2Eテスト専用のコードが含まれている:");
  for (const finding of findings) console.error(`  ${finding}`);
  process.exit(1);
}
console.log(`E2Eテスト専用のコードは含まれていない(${scanned}ファイルを確認: ${distDir})`);
