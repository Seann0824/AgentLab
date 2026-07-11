import { useEffect, useRef, useState } from "react";
import { useChatStore } from "../store/chatStore";
import { ScrollContainer } from "./ScrollContainer";

export function SettingsPanel() {
  const [namespace, setNamespace] = useState("");
  const [fileName, setFileName] = useState("");
  const [content, setContent] = useState("");
  const [isIndexing, setIsIndexing] = useState(false);
  const [message, setMessage] = useState("");
  const fileInputRef = useRef<HTMLInputElement>(null);

  const namespaces = useChatStore((s) => s.namespaces);
  const loadNamespaces = useChatStore((s) => s.loadNamespaces);
  const indexDocument = useChatStore((s) => s.indexDocument);
  const deleteNamespace = useChatStore((s) => s.deleteNamespace);

  useEffect(() => {
    loadNamespaces();
  }, [loadNamespaces]);

  const handleFileChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    setFileName(file.name);
    setMessage("");

    const reader = new FileReader();
    reader.onload = () => {
      const text = reader.result;
      if (typeof text === "string") {
        setContent(text);
      } else {
        setMessage("无法读取文件内容");
      }
    };
    reader.onerror = () => {
      setMessage("读取文件失败");
    };
    reader.readAsText(file);
  };

  const handleIndex = async () => {
    const ns = namespace.trim();
    if (!ns) {
      setMessage("请先输入 namespace");
      return;
    }
    if (!content.trim()) {
      setMessage("请先选择 Markdown 文件");
      return;
    }

    setIsIndexing(true);
    setMessage("索引中…");
    try {
      const result = await indexDocument(ns, content, fileName || "uploaded-document");
      if (result.already_exists) {
        setMessage(`namespace「${ns}」已存在相同内容的文档，无需重复上传。`);
      } else {
        setMessage(`索引完成：${ns}，共 ${result.chunks} 个 chunk。`);
        setNamespace("");
        setFileName("");
        setContent("");
        if (fileInputRef.current) {
          fileInputRef.current.value = "";
        }
      }
    } catch (err) {
      setMessage(`索引失败：${err}`);
    } finally {
      setIsIndexing(false);
    }
  };

  const handleDelete = async (ns: string) => {
    if (!confirm(`确定删除 namespace「${ns}」及其索引吗？`)) return;
    try {
      await deleteNamespace(ns);
      setMessage(`已删除 ${ns}`);
    } catch (err) {
      setMessage(`删除失败：${err}`);
    }
  };

  return (
    <ScrollContainer className="flex-1 p-8 bg-paper">
      <div className="max-w-2xl mx-auto">
        <h1 className="text-xl font-medium text-ink mb-6">知识库设置</h1>

        <section className="bg-paper-dark border border-mist rounded-lg p-6 mb-6">
          <h2 className="text-sm font-medium text-ink mb-4">上传文档</h2>

          <div className="space-y-4">
            <div>
              <label className="block text-xs text-stone mb-1">Namespace</label>
              <input
                type="text"
                value={namespace}
                onChange={(e) => setNamespace(e.currentTarget.value)}
                placeholder="例如：产品手册"
                className="input-minimal w-full py-2 px-3"
              />
            </div>

            <div>
              <label className="block text-xs text-stone mb-1">文件</label>
              <input
                ref={fileInputRef}
                type="file"
                accept=".md,.markdown,.txt"
                onChange={handleFileChange}
                disabled={isIndexing}
                className="block w-full text-sm text-ink file:mr-3 file:py-2 file:px-3 file:rounded file:border-0 file:bg-moss file:text-white hover:file:bg-moss/90"
              />
              {fileName && (
                <div className="mt-1 text-xs text-stone">已选择：{fileName}</div>
              )}
            </div>

            <button
              onClick={handleIndex}
              disabled={isIndexing || !namespace.trim() || !content.trim()}
              className="btn-moss w-full py-2 text-sm disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {isIndexing ? "索引中…" : "上传并索引"}
            </button>

            {message && (
              <div className="text-sm text-stone bg-paper border border-mist rounded px-3 py-2">
                {message}
              </div>
            )}
          </div>
        </section>

        <section className="bg-paper-dark border border-mist rounded-lg p-6">
          <h2 className="text-sm font-medium text-ink mb-4">已索引文档</h2>
          {namespaces.length === 0 ? (
            <div className="text-sm text-stone">暂无知识库</div>
          ) : (
            <ul className="space-y-2">
              {namespaces.map((ns) => (
                <li
                  key={ns}
                  className="flex items-center justify-between px-3 py-2 bg-paper border border-mist rounded"
                >
                  <span className="text-sm text-ink">{ns}</span>
                  <button
                    onClick={() => handleDelete(ns)}
                    className="text-xs text-stone hover:text-red-600 transition-colors"
                  >
                    删除
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </ScrollContainer>
  );
}
