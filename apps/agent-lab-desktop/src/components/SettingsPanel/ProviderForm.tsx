import type { ProviderConfig } from "../../types/chat";
import { PROVIDER_OPTIONS } from "./utils";

interface ProviderFormProps {
  provider: ProviderConfig;
  message: string;
  onChange: <K extends keyof ProviderConfig>(field: K, value: ProviderConfig[K]) => void;
  onUpdateModel: (index: number, value: string) => void;
  onAddModel: () => void;
  onRemoveModel: (index: number) => void;
  onSave: () => void;
  onCancel: () => void;
}

export function ProviderForm({
  provider,
  message,
  onChange,
  onUpdateModel,
  onAddModel,
  onRemoveModel,
  onSave,
  onCancel,
}: ProviderFormProps) {
  return (
    <div className="bg-paper border border-mist rounded-lg p-4 mb-4 space-y-4">
      {message && (
        <div className="text-sm text-stone bg-paper border border-mist rounded px-3 py-2">
          {message}
        </div>
      )}

      <div>
        <label className="block text-xs text-stone mb-1">显示名称</label>
        <input
          type="text"
          value={provider.name}
          onChange={(e) => onChange("name", e.currentTarget.value)}
          placeholder="例如：DeepSeek"
          className="input-minimal w-full py-2 px-3"
        />
      </div>

      <div>
        <label className="block text-xs text-stone mb-1">Provider</label>
        <select
          value={provider.provider}
          onChange={(e) => onChange("provider", e.currentTarget.value)}
          className="input-minimal w-full py-2 px-3"
        >
          {PROVIDER_OPTIONS.map((opt) => (
            <option key={opt} value={opt}>
              {opt}
            </option>
          ))}
        </select>
      </div>

      <div>
        <label className="block text-xs text-stone mb-1">Base URL</label>
        <input
          type="text"
          value={provider.base_url}
          onChange={(e) => onChange("base_url", e.currentTarget.value)}
          placeholder="https://api.deepseek.com"
          className="input-minimal w-full py-2 px-3"
        />
      </div>

      <div>
        <label className="block text-xs text-stone mb-1">API Key</label>
        <input
          type="password"
          value={provider.api_key}
          onChange={(e) => onChange("api_key", e.currentTarget.value)}
          placeholder="sk-..."
          className="input-minimal w-full py-2 px-3"
        />
        {!provider.api_key && (
          <div className="mt-1 text-xs text-amber-600">
            API Key 未填写，发送消息时会提示
          </div>
        )}
      </div>

      <div>
        <label className="block text-xs text-stone mb-1">模型列表</label>
        <div className="space-y-2">
          {provider.models.map((model, idx) => (
            <div key={idx} className="flex items-center gap-2">
              <input
                type="text"
                value={model}
                onChange={(e) => onUpdateModel(idx, e.currentTarget.value)}
                placeholder="deepseek-chat"
                className="input-minimal flex-1 py-2 px-3"
              />
              <button
                onClick={() => onRemoveModel(idx)}
                className="text-xs text-stone hover:text-red-600 transition-colors px-2"
              >
                删除
              </button>
            </div>
          ))}
          <button
            onClick={onAddModel}
            className="text-xs text-moss hover:text-moss/80 transition-colors"
          >
            + 添加模型
          </button>
        </div>
      </div>

      <div className="flex justify-end gap-3 pt-2">
        <button
          onClick={onCancel}
          className="px-4 py-2 text-sm text-stone hover:text-ink transition-colors"
        >
          取消
        </button>
        <button onClick={onSave} className="btn-moss px-4 py-2 text-sm">
          保存
        </button>
      </div>
    </div>
  );
}
