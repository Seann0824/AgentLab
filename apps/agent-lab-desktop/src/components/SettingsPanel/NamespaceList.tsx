interface NamespaceListProps {
  namespaces: string[];
  onDelete: (namespace: string) => void;
}

export function NamespaceList({ namespaces, onDelete }: NamespaceListProps) {
  if (namespaces.length === 0) {
    return <div className="text-sm text-stone">暂无知识库</div>;
  }

  return (
    <ul className="space-y-2">
      {namespaces.map((ns) => (
        <li
          key={ns}
          className="flex items-center justify-between px-3 py-2 bg-paper border border-mist rounded"
        >
          <span className="text-sm text-ink">{ns}</span>
          <button
            onClick={() => onDelete(ns)}
            className="text-xs text-stone hover:text-red-600 transition-colors"
          >
            删除
          </button>
        </li>
      ))}
    </ul>
  );
}
