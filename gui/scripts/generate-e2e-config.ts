// tauri.e2e.conf.jsonを生成する。
//
// Tauriの設定マージはJSON Merge Patch(RFC 7396)で行われ、配列はパッチ側に存在すれば
// 丸ごと置換される。そのためtauri.e2e.conf.jsonのsecurity.capabilitiesを手書きすると、
// 将来tauri.conf.json側のcapabilitiesにエントリが追加された際、こちらへの反映を
// 書き忘れてもビルドは通ってしまい、e2eビルドでその権限だけが黙って落ちる。
// ベース設定(tauri.conf.json)を唯一の情報源にし、ここから動的に組み立てることで
// この2つが分岐する余地自体を無くす。
//
// wdio:default/wdio-webdriver:defaultはe2e-testing feature配下のプラグインの権限で
// あり、通常ビルドでは対応するプラグインがコンパイルされないため、このe2e専用
// capabilityを実ファイル(src-tauri/capabilities/配下)として置くと、Tauriが
// capabilities配下の全ファイルをビルド時に検証する際に「存在しない権限」として
// 通常ビルドが壊れる。そのため--configで指定するオーバーレイ内にインライン定義する
// (このファイル自体がsrc-tauri/capabilities/配下に置かれることは無い)。

import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const srcTauriDir = join(scriptDir, "..", "src-tauri");
const baseConfigPath = join(srcTauriDir, "tauri.conf.json");
const outputPath = join(srcTauriDir, "tauri.e2e.conf.json");

interface TauriConfig {
  app?: {
    security?: {
      capabilities?: unknown[];
    };
  };
}

const baseConfig: TauriConfig = JSON.parse(readFileSync(baseConfigPath, "utf-8"));
const baseCapabilities = baseConfig.app?.security?.capabilities ?? [];

const e2eConfig = {
  app: {
    withGlobalTauri: true,
    security: {
      capabilities: [
        ...baseCapabilities,
        {
          identifier: "e2e",
          windows: ["main"],
          permissions: ["wdio:default", "wdio-webdriver:default"],
        },
      ],
    },
  },
};

writeFileSync(outputPath, `${JSON.stringify(e2eConfig, null, 2)}\n`);
console.log(`generated ${outputPath}`);
