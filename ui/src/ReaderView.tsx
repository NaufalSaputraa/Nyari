import type { VNode } from "preact";
import type { ReadingArticle } from "./store";

interface ReaderViewProps {
  article: ReadingArticle | null;
  onClose: () => void;
}

export function ReaderView({ article, onClose }: ReaderViewProps) {
  if (!article) {
    return (
      <section class="reader-container" aria-label="Reading Mode">
        <header class="reader-header">
          <strong>Reading Mode</strong>
          <button type="button" class="secondary-btn" onClick={onClose}>
            Close
          </button>
        </header>
        <div class="reader-body">
          <p>
            No extracted article is active yet. Open a web article tab and press Ctrl+Shift+R to
            extract clean blocks.
          </p>
        </div>
      </section>
    );
  }

  const renderedNodes: VNode[] = [];
  let listItems: { key: number; text: string }[] = [];

  const flushList = (keyBase: number) => {
    if (listItems.length > 0) {
      renderedNodes.push(
        <ul key={`ul-${keyBase}`}>
          {listItems.map((item) => (
            <li key={item.key}>{item.text}</li>
          ))}
        </ul>
      );
      listItems = [];
    }
  };

  article.blocks.forEach((block, index) => {
    if (block.kind === "li") {
      listItems.push({ key: index, text: block.text });
      return;
    }
    flushList(index);
    switch (block.kind) {
      case "h1":
        renderedNodes.push(<h1 key={index}>{block.text}</h1>);
        break;
      case "h2":
        renderedNodes.push(<h2 key={index}>{block.text}</h2>);
        break;
      case "p":
        renderedNodes.push(<p key={index}>{block.text}</p>);
        break;
      case "blockquote":
        renderedNodes.push(<blockquote key={index}>{block.text}</blockquote>);
        break;
      case "pre":
        renderedNodes.push(
          <pre key={index}>
            <code>{block.text}</code>
          </pre>
        );
        break;
    }
  });
  flushList(article.blocks.length);

  return (
    <section class="reader-container" aria-label="Reading Mode">
      <header class="reader-header">
        <div>
          <strong>{article.title}</strong>
          {article.byline ? <div class="palette-item-meta">{article.byline}</div> : null}
        </div>
        <button type="button" class="secondary-btn" onClick={onClose}>
          Close (Esc)
        </button>
      </header>
      <article class="reader-body">{renderedNodes}</article>
    </section>
  );
}
