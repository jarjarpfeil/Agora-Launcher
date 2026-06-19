'use client';

import ReactMarkdown from 'react-markdown';

interface MarkdownRendererProps {
  content: string;
}

export default function MarkdownRenderer({ content }: MarkdownRendererProps) {
  return (
    <ReactMarkdown
      allowedElements={['p', 'strong', 'em', 'code', 'a', 'pre', 'ul', 'ol', 'li']}
      unwrapDisallowed
    >
      {content}
    </ReactMarkdown>
  );
}
