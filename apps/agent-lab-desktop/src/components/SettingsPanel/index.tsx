import { useEffect, useMemo } from "react";
import { useChatStore } from "../../store/chatStore";
import { ConfirmDialog } from "../ConfirmDialog";
import { DropdownSelect } from "../DropdownMenu";
import { ScrollContainer } from "../ScrollContainer";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "../Accordion";
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
  const loadMemoryEnabled = useChatStore((s) => s.loadMemoryEnabled);
  const memoryEnabled = useChatStore((s) => s.memoryEnabled);
  const setMemoryEnabled = useChatStore((s) => s.setMemoryEnabled);

  useEffect(() => {
    loadNamespaces();
    loadProviders();
    loadDefaultModel();
    loadMemoryEnabled();
  }, [loadNamespaces, loadProviders, loadDefaultModel, loadMemoryEnabled]);

  const provider = useProviderManager();
  const defaultModel = useDefaultModel();
  const upload = useKnowledgeBaseUpload();
  const namespaceList = useNamespaceList();

  const displayProviders = useMemo(() => {
    if (!provider.editingProvider) {
      return provider.providers;
    }
    const exists = provider.providers.some(
      (p) => p.id === provider.editingProvider!.id,
    );
    return exists
      ? provider.providers
      : [provider.editingProvider, ...provider.providers];
  }, [provider.editingProvider, provider.providers]);

  const defaultModelOptions = useMemo(
    () => [
      { value: "", label: "选择默认模型" },
      ...defaultModel.options.map((o) => ({
        value: `${o.providerId}::${o.model}`,
        label: `${o.providerName} / ${o.model}`,
      })),
    ],
    [defaultModel.options],
  );

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

          <Accordion
            type="single"
            collapsible
            value={provider.editingProvider ? [provider.editingProvider.id] : []}
            onValueChange={(values) => {
              const id = values[0];
              if (!id) {
                provider.cancelEdit();
                return;
              }
              const p = provider.providers.find((item) => item.id === id);
              if (p) {
                provider.startEdit(p);
              }
            }}
            className="mb-4"
          >
            {provider.providers.length === 0 && !provider.editingProvider ? (
              <div className="text-sm text-stone">暂无 Provider</div>
            ) : (
              displayProviders.map((p) => (
                <AccordionItem key={p.id} value={p.id}>
                  <AccordionTrigger>
                    <div className="min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-ink truncate">
                          {p.name}
                        </span>
                        {!p.api_key && (
                          <span className="text-xs text-amber-600">未填 Key</span>
                        )}
                      </div>
                      <div className="text-xs text-stone truncate">
                        {p.base_url}
                      </div>
                      <div className="text-xs text-stone truncate">
                        {p.models.join(", ")}
                      </div>
                    </div>
                  </AccordionTrigger>
                  <AccordionContent className="px-4 py-4">
                    {provider.editingProvider?.id === p.id && (
                      <ProviderForm
                        provider={provider.editingProvider}
                        message={provider.providerMessage}
                        embedded
                        onChange={provider.updateField}
                        onUpdateModel={provider.updateModel}
                        onAddModel={provider.addModel}
                        onRemoveModel={provider.removeModel}
                        onSave={provider.handleSave}
                        onCancel={provider.cancelEdit}
                        onDelete={
                          provider.providers.some((item) => item.id === p.id)
                            ? () => provider.requestDelete(p)
                            : undefined
                        }
                      />
                    )}
                  </AccordionContent>
                </AccordionItem>
              ))
            )}
          </Accordion>

          <div>
            <label className="block text-xs text-stone mb-1">默认模型</label>
            <DropdownSelect
              value={
                defaultModel.defaultModel
                  ? `${defaultModel.defaultModel.provider_id}::${defaultModel.defaultModel.model}`
                  : ""
              }
              options={defaultModelOptions}
              onChange={(value) => {
                if (!value) return;
                const [providerId, model] = value.split("::");
                defaultModel.handleSetDefaultModel({
                  provider_id: providerId,
                  model,
                });
              }}
              fullWidth
              className="input-minimal w-full py-2 px-3 text-sm"
            />
            {defaultModel.message && (
              <div className="mt-2 text-sm text-stone">
                {defaultModel.message}
              </div>
            )}
          </div>
        </section>

        {/* 记忆设置 */}
        <h1 className="text-xl font-medium text-ink mb-6">记忆设置</h1>

        <section className="bg-paper-dark border border-mist rounded-lg p-6 mb-6">
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-sm font-medium text-ink">启用记忆记录</h2>
              <p className="text-xs text-stone mt-1">
                开启后，Agent 会根据需要调用 memory 工具保存或检索长期记忆。
              </p>
            </div>
            <button
              onClick={() => setMemoryEnabled(!memoryEnabled)}
              className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
                memoryEnabled ? "bg-moss" : "bg-stone/40"
              }`}
              aria-pressed={memoryEnabled}
            >
              <span
                className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
                  memoryEnabled ? "translate-x-6" : "translate-x-1"
                }`}
              />
            </button>
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
