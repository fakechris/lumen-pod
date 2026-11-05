/**
 * 数学定界符规范化的回归测试。
 *
 * 跑法：cd web && npm test
 *
 * 这里刻意覆盖一类容易漏掉的结构问题：`\[ ... \]` 被写在列表项 / 引用块内部时，
 * 转成 remark-math 的 `$$` 必须整体留在同一层级，否则 CommonMark 结构被撑破，
 * remark-math 会把后面的中文正文吞成公式节点，KaTeX 收到中文就整块报红。
 */
import test from "node:test";
import assert from "node:assert/strict";

import { unified } from "unified";
import remarkParse from "remark-parse";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkRehype from "remark-rehype";
import rehypeKatex from "rehype-katex";

import { normalizeMathDelimiters } from "../src/lib/markdown.ts";

const proc = unified()
  .use(remarkParse)
  .use(remarkGfm)
  .use(remarkMath)
  .use(remarkRehype)
  .use(rehypeKatex);

/** 剥掉 `\text{...}` 里的内容 —— 那里的中文是合法的，不算「正文被吞」 */
const stripText = (s) =>
  s.replace(/\\(?:text|textbf|textit|mbox|mathrm|mathbf)\{[^}]*\}/g, "");

function analyze(src) {
  const normalized = normalizeMathDelimiters(src);
  const mdast = proc.parse(normalized);

  const nodes = [];
  const walk = (n) => {
    if (!n || typeof n !== "object") return;
    if (n.type === "math") nodes.push({ kind: "block", value: n.value });
    if (n.type === "inlineMath") nodes.push({ kind: "inline", value: n.value });
    (n.children || []).forEach(walk);
  };
  walk(mdast);

  const hast = proc.runSync(mdast);
  let katexErrors = 0;
  const scan = (n) => {
    if (!n || typeof n !== "object") return;
    const cls = n.properties?.className;
    if (Array.isArray(cls) && cls.includes("katex-error")) katexErrors++;
    (n.children || []).forEach(scan);
  };
  scan(hast);

  return {
    normalized,
    nodes,
    block: nodes.filter((n) => n.kind === "block").length,
    inline: nodes.filter((n) => n.kind === "inline").length,
    katexErrors,
    // 公式节点里出现了正文（中文）—— 说明 markdown 结构被撑破了
    swallowed: nodes.filter((n) => /[一-鿿]/.test(stripText(n.value))).length,
    has: (needle) => nodes.some((n) => n.value.includes(needle)),
  };
}

function expectMath(src, want) {
  const r = analyze(src);
  assert.equal(r.katexErrors, want.katexErrors ?? 0, `katex-error 不应为 0 以外的数：\n${r.normalized}`);
  assert.equal(r.swallowed, 0, `正文被吞进了公式节点：\n${r.normalized}`);
  if (want.block !== undefined) assert.equal(r.block, want.block, `块级公式数不符：\n${r.normalized}`);
  if (want.inline !== undefined) assert.equal(r.inline, want.inline, `行内公式数不符：\n${r.normalized}`);
  if (want.has) assert.ok(r.has(want.has), `公式内容不含 ${want.has}：\n${r.normalized}`);
  return r;
}

// ---------------------------------------------------------------------------
// 定界符形态
// ---------------------------------------------------------------------------

test("块级单行公式（生产截图原样）", () => {
  expectMath("正文\n\n\\[ Pr(N=n)=2^{-n} \\]\n\n后续", {
    block: 1,
    inline: 0,
    has: "Pr(N=n)=2^{-n}",
  });
});

test("块级多行公式 + 行内公式", () => {
  expectMath("文字 \\(2\\times3\\) 结尾\n\n\\[\nE[X]=1+1+1+\\cdots=\\infty\n\\]\n", {
    block: 1,
    inline: 1,
    has: "E[X]",
  });
});

test("cases 环境（& 对齐与 \\\\ 换行）", () => {
  expectMath(
    "\\[\nM_i=\\begin{cases}1.1,& \\text{正面}\\\\0.9,& \\text{反面}\\end{cases}\n\\]",
    { block: 1, has: "begin{cases}" },
  );
});

test("行内公式", () => {
  expectMath("值是 \\(\\alpha\\) 哦", { inline: 1, has: "\\alpha" });
});

test("段落中间的块级定界符不打断段落（走行内）", () => {
  expectMath("前面文字 \\[ x=1 \\] 后面文字", {
    block: 0,
    inline: 1,
    has: "x=1",
  });
});

test("已经是 $$ 的定界符原样保留", () => {
  expectMath("已有 $$ 定界符：\n\n$$\nx=1\n$$\n", { block: 1, has: "x=1" });
});

test("代码块与行内代码里的定界符不是公式", () => {
  expectMath("```\n\\[ x \\]\n```\n\n用 `$var` 表示", { block: 0, inline: 0 });
});

// ---------------------------------------------------------------------------
// 货币保护
// ---------------------------------------------------------------------------

test("同段两个货币 $ 不被当成公式", () => {
  expectMath('Sam 说"$20 a gigawatt"，随即纠正为"$20 billion a gigawatt"。', {
    block: 0,
    inline: 0,
  });
});

test("真正的单 $ 行内公式仍然识别", () => {
  expectMath("term $x_1$ here", { inline: 1, has: "x_1" });
  expectMath("value $\\alpha$ here", { inline: 1, has: "\\alpha" });
});

// ---------------------------------------------------------------------------
// 结构：缩进（列表项内）与引用块
// ---------------------------------------------------------------------------

test("列表项内的块级公式保持缩进，且不吞后续正文", () => {
  expectMath(
    "- 事件规模越大，发生的概率按指数衰减：\n" +
      "  \\[\n" +
      "  P(X\\ge x)=e^{-\\lambda x}\n" +
      "  \\]\n" +
      "- 相应的奖金或影响按指数增长：\n" +
      "  \\[\n" +
      "  B(x)=B_0e^{gx}\n" +
      "  \\]\n" +
      "\n那么，对于一个奖金门槛 \\(b\\)，其尾部概率为：",
    { block: 2, inline: 1, has: "P(X\\ge x)=e^{-\\lambda x}" },
  );
});

test("嵌套列表（4 空格缩进）内的块级公式", () => {
  expectMath("- 外层：\n    - 内层：\n      \\[\n      y=ax+b\n      \\]\n", {
    block: 1,
    has: "y=ax+b",
  });
});

test("列表项内：定界符与正文同一行", () => {
  expectMath("- 公式：\n  \\[ y=ax+b\n  \\]\n", { block: 1, has: "y=ax+b" });
});

test("列表项内的多行块级公式", () => {
  expectMath(
    "- 公式：\n  \\[\n  P(B\\ge b)\n  =P\\left(X\\ge\\frac{\\ln(b/B_0)}{g}\\right)\n  =\\left(\\frac{b}{B_0}\\right)^{-\\lambda/g}\n  \\]\n",
    { block: 1, has: "P(B\\ge b)" },
  );
});

test("缩进块级公式之后的中文段落保持为正文", () => {
  expectMath(
    "- 公式：\n  \\[\n  y=ax+b\n  \\]\n\n这一段中文必须保持为正文，不能被吞进公式节点。\n",
    { block: 1 },
  );
});

test("顶层块级公式（缩进 0）不受影响", () => {
  expectMath("\\[\nE[L]=\\sum_i p_iD_i\n\\]\n", { block: 1, has: "E[L]" });
});

test("引用块内的块级公式", () => {
  expectMath("> 引用：\n>\n> \\[\n> x=1\n> \\]\n", { block: 1, has: "x=1" });
});

// ---------------------------------------------------------------------------
// 生产 episode 5d80e5b1 第 4 章原样回归
// ---------------------------------------------------------------------------

test("生产第 4 章原样：公式全部正确切分，正文不被吞", () => {
  const src = [
    "- 事件规模越大，发生的概率按指数衰减：",
    "  \\[",
    "  P(X\\ge x)=e^{-\\lambda x}",
    "  \\]",
    "- 相应的奖金或影响按指数增长：",
    "  \\[",
    "  B(x)=B_0e^{gx}",
    "  \\]",
    "",
    "那么，对于一个奖金门槛 \\(b\\)，其尾部概率为：",
    "",
    "\\[",
    "P(B\\ge b)",
    "=P\\left(X\\ge\\frac{\\ln(b/B_0)}{g}\\right)",
    "=\\left(\\frac{b}{B_0}\\right)^{-\\lambda/g}",
    "\\]",
    "",
    "原本关于规模 \\(x\\) 的指数衰减，变成了关于奖金 \\(b\\) 的幂律。",
    "",
    "所谓两个指数相消，最终幂指数由 \\(\\lambda/g\\) 决定。",
  ].join("\n");

  const r = expectMath(src, { block: 3, inline: 4, has: "B_0e^{gx}" });
  // 每个公式节点都必须是纯 LaTeX，不能混进中文正文
  for (const n of r.nodes) {
    assert.ok(!/[一-鿿]/.test(stripText(n.value)), `节点含中文：${n.value}`);
  }
});
