'use client';

import ReactMarkdown from 'react-markdown';
import rehypeRaw from 'rehype-raw';
import rehypeSanitize from 'rehype-sanitize';
import { defaultSchema, type Schema } from 'hast-util-sanitize';

interface MarkdownRendererProps {
  content: string;
}

// Allowlist schema for curator/upstream markdown (curator_note, body_markdown).
// rehype-sanitize's default strips <script>, on* handlers, javascript:/data:
// URLs, <iframe>. We allow richer structural tags (details/summary, tables)
// for formatting; drop `style` (blocks CSS-based UI overlay) and `className`
// (blocks Tailwind-class UI-deception injection); restrict href/src to https
// only. No dangerouslySetInnerHTML — unsafe nodes are stripped from the tree
// before React renders.
//
// MIRRORS desktop/src/pages/ModDetail.tsx SANITIZE_SCHEMA — there is no shared
// monorepo package, so keep both in sync when tightening this allowlist.
const SANITIZE_SCHEMA: Schema = {
  ...defaultSchema,
  tagNames: [
    ...(defaultSchema.tagNames ?? []),
    'details', 'summary', 'section', 'article', 'header', 'footer', 'aside',
    'figure', 'figcaption', 'mark', 'abbr', 'kbd', 'var', 'samp',
    'table', 'thead', 'tbody', 'tfoot', 'tr', 'th', 'td', 'caption', 'colgroup', 'col',
    'blockquote', 'hr', 'br', 'wbr',
  ],
  attributes: {
    ...defaultSchema.attributes,
    a: [...(defaultSchema.attributes?.a ?? []), 'title'],
    img: [...(defaultSchema.attributes?.img ?? []), 'alt', 'title', 'loading'],
    th: ['align'], td: ['align'], col: ['span'], colgroup: ['span'],
    details: ['open'],
  },
  protocols: {
    ...defaultSchema.protocols,
    href: ['https'],
    src: ['https'],
    cite: ['https'],
    poster: ['https'],
  },
};

export default function MarkdownRenderer({ content }: MarkdownRendererProps) {
  return (
    <ReactMarkdown
      rehypePlugins={[[rehypeRaw, { passThrough: ['html'] }], [rehypeSanitize, SANITIZE_SCHEMA]]}
    >
      {content}
    </ReactMarkdown>
  );
}
