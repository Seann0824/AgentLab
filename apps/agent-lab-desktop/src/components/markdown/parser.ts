import { marked, type Token, type Tokens } from "marked";

export type InlineMark = {
  bold?: boolean;
  italic?: boolean;
  strike?: boolean;
  code?: boolean;
  href?: string;
};

export type InlinePiece = {
  text: string;
  marks: InlineMark;
};

export type ParagraphBlock = {
  kind: "paragraph";
  pieces: InlinePiece[];
};

export type HeadingBlock = {
  kind: "heading";
  level: number;
  pieces: InlinePiece[];
};

export type CodeBlock = {
  kind: "code";
  text: string;
  language?: string;
};

export type ListBlock = {
  kind: "list";
  ordered: boolean;
  items: PreparedBlock[][];
};

export type BlockquoteBlock = {
  kind: "blockquote";
  blocks: PreparedBlock[];
};

export type HrBlock = {
  kind: "hr";
};

export type TableBlock = {
  kind: "table";
  header: InlinePiece[][];
  rows: InlinePiece[][][];
};

export type PreparedBlock =
  | ParagraphBlock
  | HeadingBlock
  | CodeBlock
  | TableBlock
  | ListBlock
  | BlockquoteBlock
  | HrBlock;

const EMPTY_MARKS: InlineMark = {};

function parseHref(href: string | null | undefined): string | undefined {
  if (!href) return undefined;
  try {
    const url = new URL(href);
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : undefined;
  } catch {
    return undefined;
  }
}

function collectInlinePieces(tokens: Token[], marks: InlineMark = EMPTY_MARKS): InlinePiece[] {
  const pieces: InlinePiece[] = [];

  function push(text: string, pieceMarks: InlineMark) {
    if (!text) return;
    const last = pieces[pieces.length - 1];
    if (
      last &&
      last.marks.bold === pieceMarks.bold &&
      last.marks.italic === pieceMarks.italic &&
      last.marks.strike === pieceMarks.strike &&
      last.marks.code === pieceMarks.code &&
      last.marks.href === pieceMarks.href
    ) {
      last.text += text;
    } else {
      pieces.push({ text, marks: pieceMarks });
    }
  }

  function walk(tokenList: Token[], currentMarks: InlineMark) {
    for (const token of tokenList) {
      switch (token.type) {
        case "text":
        case "escape":
          if ("tokens" in token && Array.isArray(token.tokens) && token.tokens.length > 0) {
            walk(token.tokens, currentMarks);
          } else {
            push(token.text, currentMarks);
          }
          break;
        case "strong":
          walk(token.tokens ?? [], { ...currentMarks, bold: true });
          break;
        case "em":
          walk(token.tokens ?? [], { ...currentMarks, italic: true });
          break;
        case "del":
          walk(token.tokens ?? [], { ...currentMarks, strike: true });
          break;
        case "codespan":
          push(token.text, { ...currentMarks, code: true });
          break;
        case "link":
          walk(token.tokens ?? [], { ...currentMarks, href: parseHref(token.href) });
          break;
        case "br":
          push("\n", currentMarks);
          break;
        case "html":
          push(token.text, currentMarks);
          break;
        default:
          if ("text" in token && typeof token.text === "string") {
            push(token.text, currentMarks);
          } else if (token.raw) {
            push(token.raw, currentMarks);
          }
      }
    }
  }

  walk(tokens, marks);
  return pieces;
}

function parseBlockTokens(tokens: readonly Token[]): PreparedBlock[] {
  const blocks: PreparedBlock[] = [];

  for (const token of tokens) {
    switch (token.type) {
      case "space":
      case "def":
        continue;

      case "paragraph":
        blocks.push({ kind: "paragraph", pieces: collectInlinePieces(token.tokens ?? []) });
        break;

      case "heading":
        blocks.push({
          kind: "heading",
          level: token.depth,
          pieces: collectInlinePieces(token.tokens ?? []),
        });
        break;

      case "code":
        blocks.push({ kind: "code", text: token.text, language: token.lang || undefined });
        break;

      case "list": {
        const listToken = token as Tokens.List;
        blocks.push({
          kind: "list",
          ordered: listToken.ordered,
          items: listToken.items.map((item) =>
            item.tokens.length > 0 ? parseBlockTokens(item.tokens) : [
              { kind: "paragraph", pieces: collectInlinePieces([{ type: "text", text: item.text } as Token]) },
            ]
          ),
        });
        break;
      }

      case "blockquote":
        blocks.push({ kind: "blockquote", blocks: parseBlockTokens(token.tokens ?? []) });
        break;

      case "hr":
        blocks.push({ kind: "hr" });
        break;

      case "table": {
        const tableToken = token as Tokens.Table;
        blocks.push({
          kind: "table",
          header: tableToken.header.map((cell) => collectInlinePieces(cell.tokens)),
          rows: tableToken.rows.map((row) =>
            row.map((cell) => collectInlinePieces(cell.tokens))
          ),
        });
        break;
      }

      case "html": {
        const htmlText = token.text.trim() || token.raw;
        blocks.push({ kind: "code", text: htmlText });
        break;
      }

      case "text": {
        if (Array.isArray(token.tokens) && token.tokens.length > 0) {
          blocks.push({ kind: "paragraph", pieces: collectInlinePieces(token.tokens) });
        } else {
          blocks.push({ kind: "paragraph", pieces: collectInlinePieces([token]) });
        }
        break;
      }

      default: {
        const fallback = "text" in token && typeof token.text === "string" ? token.text : token.raw;
        if (fallback) {
          blocks.push({ kind: "paragraph", pieces: collectInlinePieces([{ type: "text", text: fallback } as Token]) });
        }
      }
    }
  }

  return blocks;
}

export function parseMarkdown(content: string): PreparedBlock[] {
  if (!content.trim()) return [];
  const tokens = marked.lexer(content, { gfm: true });
  return parseBlockTokens(tokens);
}
