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
 *
 * 还有一条结构约束：`\[ ... \]` 在生产内容里常被写在列表项内部（前面带缩进），
 * 转成 `$$` 时必须把「开标记 / 正文 / 闭标记」留在同一层级，否则会撑破
 * CommonMark 的列表结构，让 remark-math 把后面的中文正文吞成公式节点 ——
 * KaTeX 收到中文就整块报红。详见 `indentBlock`。
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

/**
 * 行首的「引用标记链 + 缩进」—— 捕获组 1 是 `>` 链（如 `"> "`、`"> > "`），
 * 捕获组 2 是缩进（列表项层级）。
 *
 * 用来判断 `\[` 是否「独占一行」，以及块级公式该输出到哪一层。
 */
const LEADING_INDENT_RE = /(?:^|\n)((?:>[ \t]*)*)([ \t]*)$/;

/** `\]` 之后是否只剩空白 —— 决定块级公式是否在这一行结束 */
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

      const before = full.slice(0, offset);
      const after = full.slice(offset + match.length);

      if (display !== undefined) {
        const indentMatch = LEADING_INDENT_RE.exec(before);
        // 独占一行 → 输出块级公式（`$$` 单独成行，remark-math 才会按 flow 解析）
        if (indentMatch && LINE_END_RE.test(after)) {
          // 引用标记链 + 缩进 = 公式所属的容器层级
          const prefix = indentMatch[1] + indentMatch[2];
          const body = indentBlock(display, prefix);
          if (body.length === 0) return match;
          // `before` 末尾已经带着 prefix，所以开标记不用再写一遍
          return `$$\n${body}\n${prefix}$$`;
        }
      }

      const body = (display ?? inline ?? "").trim();
      if (body.length === 0) return match;

      // 行内公式
      return `$$${body}$$`;
    },
  );
}

/**
 * 重排块级公式正文的缩进。
 *
 * 生产内容里 `\[ ... \]` 常常写在列表项内部（前面有 2 个空格）：
 *
 * ```
 * - 事件规模越大，发生的概率按指数衰减：
 *   \[
 *   P(X\ge x)=e^{-\lambda x}
 *   \]
 * ```
 *
 * 如果把 `\[...\]` 整段替换成顶格的 `$$ ... $$`，开标记 `$$` 仍留在列表项里、
 * 正文和闭标记却跑到了列 0 —— CommonMark 的列表结构因此被撑破，remark-math
 * 随后会把列表项后面的整段中文当成公式节点吞进去，KaTeX 收到中文就整块报红。
 *
 * 所以这里统一：去掉空行、剥掉各行的引用标记与公共前导空白，再整体加上所属
 * 容器的层级前缀，保证开标记、正文、闭标记三者处于同一层。
 */
function indentBlock(body: string, prefix: string): string {
  const lines = body
    .split("\n")
    .map((line) => line.replace(/[ \t]+$/, ""))
    // 剥掉行首的引用标记（`>` 链），让正文回到容器内部
    .map((line) => {
      const quoted = /^((?:>[ \t]*)+)/.exec(line);
      return quoted ? line.slice(quoted[1].length) : line;
    })
    .filter((line) => line.trim().length > 0);
  if (lines.length === 0) return "";

  const common = lines.reduce((min, line) => {
    const lead = /^[ \t]*/.exec(line)?.[0] ?? "";
    return lead.length < min.length ? lead : min;
  }, /^[ \t]*/.exec(lines[0])?.[0] ?? "");

  return lines.map((line) => prefix + line.slice(common.length)).join("\n");
}
