import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

// Links open externally; everything else is styled via .md-content (index.css).
const components = {
  a: (props) => <a {...props} target="_blank" rel="noreferrer" />,
};

export default function Markdown({ children }) {
  return (
    <div className="md-content select-text">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={components}>
        {children || ""}
      </ReactMarkdown>
    </div>
  );
}
