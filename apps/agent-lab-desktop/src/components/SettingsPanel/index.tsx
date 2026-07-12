import { useEffect } from "react";
import { useChatStore } from "../../store/chatStore";
import { ConfirmDialog } from "../ConfirmDialog";
import { ScrollContainer } from "../ScrollContainer";
import { NamespaceList } from "./NamespaceList";
import { ProviderForm } from "./ProviderForm";
import { useDefaultModel } from "./useDefaultModel";
import { useKnowledgeBaseUpload } from "./useKnowledgeBaseUpload";
import { useNamespaceList } from "./useNamespaceList";
import { useProviderManager } from "./useProviderManager";

export function SettingsPanel() {
  const loadNamespaces = useChatStore((s) => s.loadNamespaces);
  const loadProviders = useChatStore((s) => s.loadProviders);
  const loadDefaultModel = useChatStore((s) => s.loadDefaultModel);

  useEffect(() => {
    loadNamespaces();
    loadProviders();
    loadDefaultModel();
  }, [loadNamespaces, loadProviders, loadDefaultModel]);

  const provider = useProviderManager();
  const defaultModel = useDefaultModel();
  const upload = useKnowledgeBaseUpload();
  const namespaceList = useNamespaceList();

  return (
    <ScrollContainer className="flex-1 p-8 bg-paper">
      <div className="max-w-2xl mx-auto">
        <h1 className="text-xl font-medium text-ink mb-6">设置</h1>

        {/* 模型配置 */}
        <section className="bg-paper-dark border border-mist rounded-lg p-6 mb-6">
          <div className="flex items-center justify-between mb-4">
            <h2 className="text-sm font-medium text-ink">模型配置</h2>
            <button
              onClick={provider.startCreate}
              className="text-xs text-moss hover:text-moss/80 transition-colors"
            >
              + 新增 Provider
            </button>
          </div>

          {provider.editingProvider && (
            <ProviderForm
              provider={provider.editingProvider}
              message={provider.providerMessage}
              onChange={provider.updateField}
              onUpdateModel={provider.updateModel}
              onAddModel={provider.addModel}
              onRemoveModel={provider.removeModel}
              onSave={provider.handleSave}
              onCancel={provider.cancelEdit}
            />
          )}

          <div className="space-y-2 mb-4">
            {provider.providers.length === 0 ? (
              <div className="text-sm text-stone">暂无 Provider</div>
            ) : (
              provider.providers.map((p) => (
                <div
                  key={p.id}
                  className="flex items-center justify-between px-3 py-2 bg-paper border border-mist rounded"
                >
                  <div className="min-w-0">
                    <div className="text-sm text-ink truncate">
                      {p.name}
                      {!p.api_key && (
                        <span className="ml-2 text-xs text-amber-600">未填 Key</span>
                      )}
                    </div>
                    <div className="text-xs text-stone truncate">{p.base_url}</div>
                    <div className="text-xs text-stone">{p.models.join(", ")}</div>
                  </div>
                  <div className="flex items-center gap-2 flex-shrink-0">
                    <button
                      onClick={() => provider.startEdit(p)}
                      className="text-xs text-stone hover:text-moss transition-colors px-2"
                    >
                      编辑
                    </button>
                    <button
                      onClick={() => provider.requestDelete(p)}
                      className="text-xs text-stone hover:text-red-600 transition-colors px-2"
                    >
                      删除
                    </button>
                  </div>
                </div>
              ))
            )}
          </div>

          <div>
            <label className="block text-xs text-stone mb-1">默认模型</label>
            <select
              value={
                defaultModel.defaultModel
                  ? `${defaultModel.defaultModel.provider_id}::${defaultModel.defaultModel.model}`
                  : ""
              }
              onChange={(e) => {
                const value = e.currentTarget.value;
                if (!value) return;
                const [providerId, model] = value.split("::");
                defaultModel.handleSetDefaultModel({ provider_id: providerId, model });
              }}
              className="input-minimal w-full py-2 px-3"
            >
              <option value="">选择默认模型</option>
              {defaultModel.options.map(({ providerId, providerName, model }) => (
                <option key={`${providerId}::${model}`} value={`${providerId}::${model}`}>
                  {providerName} / {model}
                </option>
              ))}
            </select>
            {defaultModel.message && (
              <div className="mt-2 text-sm text-stone">{defaultModel.message}</div>
            )}
          </div>
        </section>

        {/* 知识库 */}
        <h1 className="text-xl font-medium text-ink mb-6">知识库设置</h1>

        <section className="bg-paper-dark border border-mist rounded-lg p-6 mb-6">
          <h2 className="text-sm font-medium text-ink mb-4">上传文档</h2>

          <div className="space-y-4">
            <div>
              <label className="block text-xs text-stone mb-1">Namespace</label>
              <input
                type="text"
                value={upload.namespace}
                onChange={(e) => upload.setNamespace(e.currentTarget.value)}
                placeholder="例如：产品手册"
                className="input-minimal w-full py-2 px-3"
              />
            </div>

            <div>
              <label className="block text-xs text-stone mb-1">文件</label>
              <input
                ref={upload.fileInputRef}
                type="file"
                accept=".md,.markdown,.txt"
                onChange={upload.handleFileChange}
                disabled={upload.isIndexing}
                className="block w-full text-sm text-ink file:mr-3 file:py-2 file:px-3 file:rounded file:border-0 file:bg-moss file:text-white hover:file:bg-moss/90"
              />
              {upload.fileName && (
                <div className="mt-1 text-xs text-stone">已选择：{upload.fileName}</div>
              )}
            </div>

            <button
              onClick={upload.handleIndex}
              disabled={!upload.canIndex}
              className="btn-moss w-full py-2 text-sm disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {upload.isIndexing ? "索引中…" : "上传并索引"}
            </button>

            {upload.message && (
              <div className="text-sm text-stone bg-paper border border-mist rounded px-3 py-2">
                {upload.message}
              </div>
            )}
          </div>
        </section>

        <section className="bg-paper-dark border border-mist rounded-lg p-6">
          <h2 className="text-sm font-medium text-ink mb-4">已索引文档</h2>
          <NamespaceList
            namespaces={namespaceList.namespaces}
            onDelete={namespaceList.requestDelete}
          />
          {namespaceList.message && (
            <div className="mt-4 text-sm text-stone">{namespaceList.message}</div>
          )}
        </section>
      </div>

      <ConfirmDialog
        isOpen={namespaceList.deletingNamespace !== null}
        title="删除知识库"
        message={
          namespaceList.deletingNamespace
            ? `确定删除 namespace「${namespaceList.deletingNamespace}」及其索引吗？删除后无法恢复。`
            : ""
        }
        confirmText="删除"
        onConfirm={namespaceList.confirmDelete}
        onCancel={namespaceList.cancelDelete}
      />

      <ConfirmDialog
        isOpen={provider.deletingProvider !== null}
        title="删除 Provider"
        message={
          provider.deletingProvider
            ? `确定删除 Provider「${provider.deletingProvider.name}」吗？删除后无法恢复。`
            : ""
        }
        confirmText="删除"
        onConfirm={provider.confirmDelete}
        onCancel={provider.cancelDelete}
      />
    </ScrollContainer>
  );
}
