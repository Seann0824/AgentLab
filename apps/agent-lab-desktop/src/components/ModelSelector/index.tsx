import {
  Dropdown,
  DropdownMenu,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownTrigger,
} from "../DropdownMenu";
import type { ModelGroup } from "../ChatInput/useModelSelector";

interface ModelSelectorProps {
  groups: ModelGroup[];
  currentKey: string;
  onChange: (key: string) => void;
  disabled?: boolean;
}

export function ModelSelector({
  groups,
  currentKey,
  onChange,
  disabled,
}: ModelSelectorProps) {
  const selectedLabel = getSelectedLabel(groups, currentKey);

  return (
    <Dropdown>
      <DropdownTrigger disabled={disabled}>
        {selectedLabel}
      </DropdownTrigger>
      <DropdownMenu placement="top" align="right" className="min-w-[220px] max-w-md">
        <DropdownMenuItem active={currentKey === ""} onClick={() => onChange("")}>
          默认模型
        </DropdownMenuItem>
        {groups.map((group) => (
          <DropdownMenuGroup key={group.providerId} title={group.providerName}>
            {group.models.map((model) => (
              <DropdownMenuItem
                key={model.key}
                active={model.key === currentKey}
                onClick={() => onChange(model.key)}
              >
                {model.label}
              </DropdownMenuItem>
            ))}
          </DropdownMenuGroup>
        ))}
      </DropdownMenu>
    </Dropdown>
  );
}

function getSelectedLabel(groups: ModelGroup[], currentKey: string): string {
  if (currentKey === "") return "默认模型";
  for (const group of groups) {
    const model = group.models.find((m) => m.key === currentKey);
    if (model) {
      return `${group.providerName} / ${model.label}`;
    }
  }
  return "未知模型";
}
