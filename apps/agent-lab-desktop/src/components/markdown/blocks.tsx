import { useMemo, useRef, useLayoutEffect, useState, type RefObject } from "react";
import {
  layoutWithLines,
  prepareWithSegments,
  type LayoutLine,
  type PreparedTextWithSegments,
} from "@chenglou/pretext";
import {
  walkRichInlineLineRanges,
  materializeRichInlineLineRange,
  prepareRichInline,
  type PreparedRichInline,
  type RichInlineLine,
  type RichInlineLineRange,
} from "@chenglou/pretext/rich-inline";
import { ScrollContainer } from "../ScrollContainer";
import { parseMarkdown } from "./parser";
import type {
  PreparedBlock,
  InlinePiece,
  ParagraphBlock,
  HeadingBlock,
  CodeBlock,
  ListBlock,
  BlockquoteBlock,
  TableBlock,
} from "./parser";

const SANS_FAMILY =
  '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", sans-serif';
const MONO_FAMILY = 'ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace';

const BODY_LINE_HEIGHT = 22;
const HEADING_1_LINE_HEIGHT = 28;
const HEADING_2_LINE_HEIGHT = 25;
const CODE_LINE_HEIGHT = 18;
const CODE_PADDING_X = 12;
const CODE_PADDING_Y = 8;

function resolvePieceFont(piece: InlinePiece, variant: "body" | "heading-1" | "heading-2"): string {
  const italic = piece.marks.italic ? "italic " : "";
  switch (variant) {
    case "heading-1": {
      const weight = piece.marks.bold ? 800 : 700;
      return `${italic}${weight} 20px ${SANS_FAMILY}`;
    }
    case "heading-2": {
      const weight = piece.marks.bold ? 700 : 600;
      return `${italic}${weight} 17px ${SANS_FAMILY}`;
    }
    case "body": {
      const weight = piece.marks.bold ? 700 : piece.marks.href ? 500 : 400;
      return `${italic}${weight} 14px ${SANS_FAMILY}`;
    }
  }
}

function resolvePieceClassName(piece: InlinePiece, variant: "body" | "heading-1" | "heading-2"): string {
  const base = variant === "heading-1"
    ? "text-xl font-bold text-ink"
    : variant === "heading-2"
    ? "text-lg font-semibold text-ink"
    : "text-sm text-ink-light";

  if (piece.marks.code) return `${base} px-1 py-0.5 bg-paper border border-mist rounded-sm font-mono text-xs`;
  if (piece.marks.href) return `${base} text-moss hover:underline cursor-pointer`;
  if (piece.marks.strike) return `${base} line-through`;
  return base;
}

function lineHeightForVariant(variant: "body" | "heading-1" | "heading-2"): number {
  switch (variant) {
    case "heading-1":
      return HEADING_1_LINE_HEIGHT;
    case "heading-2":
      return HEADING_2_LINE_HEIGHT;
    case "body":
    default:
      return BODY_LINE_HEIGHT;
  }
}

type InlineBlockProps = {
  pieces: InlinePiece[];
  variant: "body" | "heading-1" | "heading-2";
  maxWidth: number;
  className?: string;
};

function InlineBlock({ pieces, variant, maxWidth, className = "" }: InlineBlockProps) {
  const prepared: PreparedRichInline = useMemo(() => {
    return prepareRichInline(
      pieces.map((piece) => ({
        text: piece.text,
        font: resolvePieceFont(piece, variant),
        break: "normal",
        extraWidth: piece.marks.code ? 8 : 0,
      }))
    );
  }, [pieces, variant]);

  const lines: RichInlineLine[] = useMemo(() => {
    const result: RichInlineLine[] = [];
    walkRichInlineLineRanges(prepared, Math.max(1, maxWidth), (range: RichInlineLineRange) => {
      result.push(materializeRichInlineLineRange(prepared, range));
    });
    return result;
  }, [prepared, maxWidth]);

  const lineHeight = lineHeightForVariant(variant);

  return (
    <div className={className} style={{ lineHeight: `${lineHeight}px` }}>
      {lines.map((line, lineIndex) => (
        <div key={lineIndex} className="whitespace-nowrap">
          {line.fragments.map((fragment, fragmentIndex) => {
            const piece = pieces[fragment.itemIndex];
            if (!piece) return null;
            return (
              <span
                key={fragmentIndex}
                className={resolvePieceClassName(piece, variant)}
                style={{ marginLeft: fragment.gapBefore > 0 ? `${fragment.gapBefore}px` : undefined }}
              >
                {fragment.text}
              </span>
            );
          })}
        </div>
      ))}
    </div>
  );
}

function ParagraphBlockComponent({ block, maxWidth }: { block: ParagraphBlock; maxWidth: number }) {
  return (
    <InlineBlock
      pieces={block.pieces}
      variant="body"
      maxWidth={maxWidth}
      className="my-2"
    />
  );
}

function HeadingBlockComponent({ block, maxWidth }: { block: HeadingBlock; maxWidth: number }) {
  const variant = block.level <= 1 ? "heading-1" : "heading-2";
  return (
    <InlineBlock
      pieces={block.pieces}
      variant={variant}
      maxWidth={maxWidth}
      className={block.level <= 1 ? "mt-4 mb-2" : "mt-3 mb-2"}
    />
  );
}

function CodeBlockComponent({ block, maxWidth }: { block: CodeBlock; maxWidth: number }) {
  const prepared: PreparedTextWithSegments = useMemo(() => {
    return prepareWithSegments(
      block.text.endsWith("\n") ? block.text.slice(0, -1) : block.text,
      `500 12px ${MONO_FAMILY}`,
      { whiteSpace: "pre-wrap" }
    );
  }, [block.text]);

  const lines = useMemo(() => {
    const innerWidth = Math.max(1, maxWidth - CODE_PADDING_X * 2);
    const result = layoutWithLines(prepared, innerWidth, CODE_LINE_HEIGHT);
    return result.lines;
  }, [prepared, maxWidth]);

  return (
    <ScrollContainer direction="horizontal" className="my-3">
      <pre
        className="bg-paper-deep border border-mist rounded-sm text-xs font-mono"
        style={{ padding: `${CODE_PADDING_Y}px ${CODE_PADDING_X}px` }}
      >
        <code className="block" style={{ lineHeight: `${CODE_LINE_HEIGHT}px` }}>
          {lines.map((line: LayoutLine, index: number) => (
            <div key={index} className="whitespace-pre">
              {line.text}
            </div>
          ))}
        </code>
      </pre>
    </ScrollContainer>
  );
}

function TableBlockComponent({ block, maxWidth }: { block: TableBlock; maxWidth: number }) {
  const cellWidth = Math.max(1, Math.floor(maxWidth / Math.max(1, block.header.length)));

  return (
    <ScrollContainer direction="horizontal" className="my-3">
      <table className="w-full text-sm border-collapse border border-mist bg-white shadow-soft">
        <thead className="bg-paper-dark">
          <tr>
            {block.header.map((cellPieces, colIndex) => (
              <th
                key={colIndex}
                className="border border-mist px-3 py-2 text-left font-semibold text-ink"
              >
                <InlineBlock pieces={cellPieces} variant="body" maxWidth={cellWidth} className="" />
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {block.rows.map((row, rowIndex) => (
            <tr key={rowIndex}>
              {row.map((cellPieces, colIndex) => (
                <td
                  key={colIndex}
                  className="border border-mist px-3 py-2 text-ink-light"
                >
                  <InlineBlock pieces={cellPieces} variant="body" maxWidth={cellWidth} className="" />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </ScrollContainer>
  );
}

function ListBlockComponent({ block, maxWidth }: { block: ListBlock; maxWidth: number }) {
  const innerWidth = Math.max(1, maxWidth - 20);
  return (
    <div className="my-2 pl-1">
      {block.items.map((itemBlocks, itemIndex) => (
        <div key={itemIndex} className="flex gap-2">
          <span className="text-ink-light text-sm select-none w-4 text-right">
            {block.ordered ? `${itemIndex + 1}.` : "•"}
          </span>
          <div className="flex-1">
            {itemBlocks.map((subBlock, subIndex) => (
              <BlockRenderer key={subIndex} block={subBlock} maxWidth={innerWidth} />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

function BlockquoteBlockComponent({ block, maxWidth }: { block: BlockquoteBlock; maxWidth: number }) {
  return (
    <div className="my-3 pl-3 border-l-3 border-moss">
      {block.blocks.map((subBlock, index) => (
        <BlockRenderer key={index} block={subBlock} maxWidth={Math.max(1, maxWidth - 12)} />
      ))}
    </div>
  );
}

function HrBlockComponent() {
  return <hr className="my-4 border-mist" />;
}

function BlockRenderer({ block, maxWidth }: { block: PreparedBlock; maxWidth: number }) {
  switch (block.kind) {
    case "paragraph":
      return <ParagraphBlockComponent block={block} maxWidth={maxWidth} />;
    case "heading":
      return <HeadingBlockComponent block={block} maxWidth={maxWidth} />;
    case "code":
      return <CodeBlockComponent block={block} maxWidth={maxWidth} />;
    case "table":
      return <TableBlockComponent block={block} maxWidth={maxWidth} />;
    case "list":
      return <ListBlockComponent block={block} maxWidth={maxWidth} />;
    case "blockquote":
      return <BlockquoteBlockComponent block={block} maxWidth={maxWidth} />;
    case "hr":
      return <HrBlockComponent />;
  }
}

function blockKey(block: PreparedBlock): string {
  switch (block.kind) {
    case "paragraph":
      return `p:${block.pieces.map((p) => p.text).join("")}`;
    case "heading":
      return `h${block.level}:${block.pieces.map((p) => p.text).join("")}`;
    case "code":
      return `code:${block.language || ""}:${block.text}`;
    case "table":
      return `table:${block.header.map((c) => c.map((p) => p.text).join("")).join("|")}:${block.rows.map((r) => r.map((c) => c.map((p) => p.text).join("")).join("|")).join(";")}`;
    case "list":
      return `list:${block.ordered}:${block.items.map((item) => item.map(blockKey).join(",")).join("|")}`;
    case "blockquote":
      return `blockquote:${block.blocks.map(blockKey).join("|")}`;
    case "hr":
      return "hr";
  }
}

function useContainerWidth<T extends HTMLElement>(): [RefObject<T | null>, number] {
  const ref = useRef<T | null>(null);
  const [width, setWidth] = useState(0);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;

    const update = () => setWidth(el.clientWidth);
    update();

    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  return [ref, width];
}

export function PretextMarkdown({ content }: { content: string }) {
  const [containerRef, width] = useContainerWidth<HTMLDivElement>();
  const blockCacheRef = useRef<Map<string, PreparedBlock>>(new Map());

  const blocks = useMemo(() => {
    const freshBlocks = parseMarkdown(content);
    const cache = blockCacheRef.current;
    return freshBlocks.map((block) => {
      const key = blockKey(block);
      const cached = cache.get(key);
      if (cached) return cached;
      cache.set(key, block);
      return block;
    });
  }, [content]);

  return (
    <div ref={containerRef} className="w-full">
      {width > 0 && blocks.map((block, index) => (
        <BlockRenderer key={`${index}-${blockKey(block)}`} block={block} maxWidth={width} />
      ))}
    </div>
  );
}
