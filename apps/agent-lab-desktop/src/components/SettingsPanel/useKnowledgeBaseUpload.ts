import { useCallback, useRef, useState } from "react";
import { useChatStore } from "../../store/chatStore";

export interface UseKnowledgeBaseUploadReturn {
  namespace: string;
  setNamespace: (value: string) => void;
  fileName: string;
  content: string;
  isIndexing: boolean;
  message: string;
  fileInputRef: React.RefObject<HTMLInputElement | null>;
  handleFileChange: (event: React.ChangeEvent<HTMLInputElement>) => void;
  handleIndex: () => Promise<void>;
  canIndex: boolean;
}

export function useKnowledgeBaseUpload(): UseKnowledgeBaseUploadReturn {
  const [namespace, setNamespace] = useState("");
  const [fileName, setFileName] = useState("");
  const [content, setContent] = useState("");
  const [isIndexing, setIsIndexing] = useState(false);
  const [message, setMessage] = useState("");
  const fileInputRef = useRef<HTMLInputElement>(null);

  const indexDocument = useChatStore((s) => s.indexDocument);
  const loadNamespaces = useChatStore((s) => s.loadNamespaces);

  const handleFileChange = useCallback(
    (event: React.ChangeEvent<HTMLInputElement>) => {
      const file = event.target.files?.[0];
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
    },
    [],
  );

  const handleIndex = useCallback(async () => {
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
        await loadNamespaces();
      }
    } catch (err) {
      setMessage(`索引失败：${err}`);
    } finally {
      setIsIndexing(false);
    }
  }, [namespace, content, fileName, indexDocument, loadNamespaces]);

  const canIndex =
    !isIndexing && namespace.trim().length > 0 && content.trim().length > 0;

  return {
    namespace,
    setNamespace,
    fileName,
    content,
    isIndexing,
    message,
    fileInputRef,
    handleFileChange,
    handleIndex,
    canIndex,
  };
}
