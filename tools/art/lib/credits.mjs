// Writes docs/credits.md (Korean, like the rest of docs/) from the LICENSES.json the build made. A
// pack with an attribution (a CC-BY pack must have one) gets it shown with the pack and each file.

const escape = (text) => String(text).replace(/\|/g, "\\|");

export function creditsMarkdown(licenses) {
  const packs = Object.entries(licenses.packs).sort(([a], [b]) => a.localeCompare(b));
  const files = Object.entries(licenses.files).sort(([a], [b]) => a.localeCompare(b));
  const attributed = packs.filter(([, pack]) => pack.attribution);
  const lines = [
    "# 크레딧",
    "",
    "이 파일은 `tools/art/build-art.mjs`가 `client/public/art/LICENSES.json`에서 만듭니다. 손으로 고치지 말고, 레시피(`tools/art/recipes/`)를 고친 뒤 다시 실행하세요.",
    "",
    "게임의 모델·텍스처·이펙트 이미지는 아래 팩에서 가져와 변환했습니다. 사운드는 코드로 합성하고, 그 밖의 그래픽(하늘, 이름표, 광고판 글자, 벽·바닥 텍스처 등)도 코드로 그립니다.",
    "",
    "## 팩",
    "",
    "| 팩 | 저작자 | 라이선스 | 출처 |",
    "| --- | --- | --- | --- |",
    ...packs.map(([, pack]) => `| ${escape(pack.title)} | ${escape(pack.author)} | ${escape(pack.license)} (${escape(pack.licenseText ?? pack.license)}) | ${escape(pack.url)} |`),
    "",
    "CC0 팩은 표기 의무가 없지만, 저작자에게 감사하는 뜻으로 적습니다.",
    "",
  ];
  if (attributed.length) {
    lines.push("## 표기", "", "표기 문구(attribution)가 있는 팩은 아래와 같이 표기합니다. CC-BY 팩은 라이선스에 따라 반드시 표기합니다.", "");
    for (const [, pack] of attributed) lines.push(`- ${pack.title} (${pack.license}): ${pack.attribution}`);
    lines.push("");
  }
  lines.push("## 파일", "");
  for (const [path, file] of files) {
    const pack = licenses.packs[file.pack];
    lines.push(`### \`${path}\``, "", `- 팩: ${pack.title} (${pack.author}, ${pack.license})`);
    if (pack.attribution) lines.push(`- 표기: ${pack.attribution}`);
    lines.push(`- 원본: ${file.sourcePath.split(", ").map((source) => `\`${source}\``).join(", ")}`, "- 바꾼 점:");
    for (const change of file.changes) lines.push(`  - ${change}`);
    lines.push("");
  }
  return lines.join("\n");
}
