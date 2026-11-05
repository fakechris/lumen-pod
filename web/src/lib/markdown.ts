/**
 * Markdown 数学定界符规范化。
 *
 * 背景：生产内容里 LLM 写的是 LaTeX 经典定界符 —— 块级 `\[ ... \]`、行内 `\( ... \)`，
 * 而 remark-math 只认 `$` / `$$`。不先做规范化，这两种形态一个都不会被渲染，
 * 页面上会直接露出 LaTeX 源码。
 *
 * 另外正文里存在货币金额（实测同一段里就有 `"$20 a gigawatt"` 与 `"$20 billion"`
 * 两个 `$`）。开启 `singleDollarTextMath` 时整段会被误判成公式，因此这里把
 * **紧跟数字的 `$`** 转义成字面量：货币保持文本，真正的行内公式（`$x_1$`）仍按公式渲染。
 *
 * 代码块与行内代码原样保留 —— 里面出现的 `\[` / `$` 是代码，不是公式。
 */

/** 围栏代码块 / 行内代码：这些片段整体跳过公式处理 */
const CODE_SEGMENT_RE = /(```[\s\S]*?(?:```|$)|~~~[\s\S]*?(?:~~~|$)|`[^`\n]*`)/g;

/**
 * 依次匹配（顺序重要，先命中的先处理）：
 * 1. `$$ ... $$` —— 已经是 remark-math 的定界符，原样保留
 * 2. `\[ ... \]` —— LaTeX 块级公式（捕获组只取公式体，不含定界符）
 * 3. `\( ... \)` —— LaTeX 行内公式（同上）
 * 4. `$` + 数字 —— 货币金额，转义成字面量
 */
const MATH_SEGMENT_RE =
  /(\$\$[\s\S]*?\$\$)|(?:\\\[)([\s\S]*?)(?:\\\])|(?:\\\()([\s\S]*?)(?:\\\))|(\$(?=\d))/g;

/** `\[` / `\(` 是否独占一行 —— 决定输出块级公式还是行内公式 */
const LINE_START_RE = /(?:^|\n)[ \t]*$/;
const LINE_END_RE = /^[ \t]*(?:\n|$)/;

/**
 * 把 LaTeX 经典定界符转换成 remark-math 能识别的 `$` / `$$`。
 *
 * - `\[ ... \]` 独占一行 → `$$` 单独成行的 flow 语法（块级公式，独立成行居中）
 * - `\[ ... \]` 出现在段落中间 → `$$ ... $$` 行内公式，不打断段落
 * - `\( ... \)` → `$$ ... $$` 行内公式
 * - `$123` → `\$123`，货币保持字面量
 */
export function normalizeMathDelimiters(markdown: string): string {
  const segments = markdown.split(CODE_SEGMENT_RE);
  return segments
    .map((segment, index) =>
      // split 带捕获组，奇数位是代码段，原样返回
      index % 2 === 1 ? segment : normalizeMathSegment(segment),
    )
    .join("");
}

function normalizeMathSegment(text: string): string {
  return text.replace(
    MATH_SEGMENT_RE,
    (
      match: string,
      dollars: string | undefined,
      display: string | undefined,
      inline: string | undefined,
      currency: string | undefined,
      offset: number,
      full: string,
    ) => {
      // `$$...$$` 已是 remark-math 语法，不动
      if (dollars !== undefined) return match;
      // `$` + 数字 → 货币，转义成字面量
      if (currency !== undefined) return `\\${match}`;

      const body = (display ?? inline ?? "").trim();
      if (body.length === 0) return match;

      if (display !== undefined) {
        const before = full.slice(0, offset);
        const after = full.slice(offset + match.length);
        // 独占一行 → 输出块级公式（`$$` 单独成行，remark-math 才会按 flow 解析）
        if (LINE_START_RE.test(before) && LINE_END_RE.test(after)) {
          return `$$\n${body}\n$$`;
        }
      }

      // 行内公式
      return `$$${body}$$`;
    },
  );
}
