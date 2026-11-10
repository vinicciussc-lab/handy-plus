import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderOpen } from "lucide-react";
import {
  commands,
  type CloudModel,
  type CorrectionSettings,
  type LocalModel,
  type ModelUnloadTimeout,
} from "@/bindings";
import { Dropdown, SettingContainer, SettingsGroup } from "@/components/ui";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { useSettings } from "../../../hooks/useSettings";
import { notify } from "@/stores/noticeStore";
import { ApiKeyField } from "../PostProcessingSettingsApi/ApiKeyField";

const LOCAL = "local";
const NONE = "";
// Apple Intelligence only exists on a Mac, and has no model or key to set.
const APPLE_PROVIDER_ID = "apple_intelligence";

// The values the backend sends and accepts, like ModelUnloadTimeout.tsx.
const TIMEOUT_VALUES = [
  "never",
  "immediately",
  "min2",
  "min5",
  "min10",
  "min15",
  "hour1",
] as const;

const formatGb = (bytes: number, language: string) =>
  new Intl.NumberFormat(language, {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  }).format(bytes / 1024 ** 3);

interface CloudFieldsProps {
  value: CloudModel;
  onModel: (model: string) => void;
}

// The model name and the key of one cloud provider. Keys are the ones the
// dictation post-processing already stores, so they're entered only once.
const CloudFields: React.FC<CloudFieldsProps> = ({ value, onModel }) => {
  const { t } = useTranslation();
  const { settings, updatePostProcessApiKey, isUpdating } = useSettings();
  const apiKey = settings?.post_process_api_keys?.[value.provider_id] ?? "";
  const [model, setModel] = useState(value.model);

  useEffect(() => {
    setModel(value.model);
  }, [value.model, value.provider_id]);

  return (
    <>
      <SettingContainer
        title={t("settings.correction.cloud.model.title")}
        description={t("settings.correction.cloud.model.description")}
        descriptionMode="tooltip"
        layout="horizontal"
        grouped={true}
      >
        <Input
          type="text"
          value={model}
          onChange={(event) => setModel(event.target.value)}
          onBlur={() => {
            const trimmed = model.trim();
            setModel(trimmed);
            if (trimmed !== value.model) onModel(trimmed);
          }}
          placeholder={t("settings.correction.cloud.model.placeholder")}
          variant="compact"
          className="min-w-[320px]"
        />
      </SettingContainer>
      <SettingContainer
        title={t("settings.correction.cloud.apiKey.title")}
        description={t("settings.correction.cloud.apiKey.description")}
        descriptionMode="tooltip"
        layout="horizontal"
        grouped={true}
      >
        <ApiKeyField
          value={apiKey}
          onBlur={(next) => {
            const trimmed = next.trim();
            if (trimmed !== apiKey) {
              void updatePostProcessApiKey(value.provider_id, trimmed);
            }
          }}
          placeholder={t("settings.correction.cloud.apiKey.placeholder")}
          disabled={isUpdating(`post_process_api_key:${value.provider_id}`)}
          className="min-w-[320px]"
        />
      </SettingContainer>
    </>
  );
};

interface CorrectionProviderSectionProps {
  settings: CorrectionSettings;
  onChange: (patch: Partial<CorrectionSettings>) => void;
}

export const CorrectionProviderSection: React.FC<
  CorrectionProviderSectionProps
> = ({ settings, onChange }) => {
  const { t, i18n } = useTranslation();
  const { settings: appSettings } = useSettings();
  const [dirDraft, setDirDraft] = useState(settings.models_dir);
  const [models, setModels] = useState<LocalModel[]>([]);

  const providers = useMemo(
    () =>
      (appSettings?.post_process_providers ?? []).filter(
        (provider) => provider.id !== APPLE_PROVIDER_ID,
      ),
    [appSettings?.post_process_providers],
  );
  const savedModels = appSettings?.post_process_models ?? {};

  const cloud = settings.provider.kind === "cloud" ? settings.provider : null;
  const isLocal = cloud === null;

  useEffect(() => {
    setDirDraft(settings.models_dir);
  }, [settings.models_dir]);

  // The list follows the folder, and starts empty when the folder is gone.
  useEffect(() => {
    if (!isLocal) return;
    let cancelled = false;
    commands
      .listLocalModels(settings.models_dir)
      .then((found) => {
        if (!cancelled) setModels(found);
      })
      .catch((error) => {
        if (cancelled) return;
        setModels([]);
        notify({
          title: t("settings.correction.local.listFailed"),
          detail: String(error),
        });
      });
    return () => {
      cancelled = true;
    };
  }, [isLocal, settings.models_dir, t]);

  const providerOptions = [
    { value: LOCAL, label: t("settings.correction.provider.local") },
    ...providers.map((provider) => ({
      value: provider.id,
      label: provider.label,
    })),
  ];

  const providerValue = cloud ? cloud.provider_id : LOCAL;

  const handleProvider = (value: string) => {
    if (value === providerValue) return;
    if (value === LOCAL) {
      onChange({ provider: { kind: "local" } });
    } else {
      onChange({
        provider: {
          kind: "cloud",
          provider_id: value,
          model: savedModels[value] ?? "",
        },
      });
    }
  };

  const commitDir = () => {
    const trimmed = dirDraft.trim();
    setDirDraft(trimmed);
    if (trimmed !== settings.models_dir) onChange({ models_dir: trimmed });
  };

  const pickFolder = async () => {
    try {
      const chosen = await open({
        directory: true,
        multiple: false,
        defaultPath: settings.models_dir || undefined,
      });
      if (typeof chosen === "string") onChange({ models_dir: chosen });
    } catch (error) {
      notify({
        title: t("settings.correction.local.pickFailed"),
        detail: String(error),
      });
    }
  };

  const modelOptions = models.map((model) => ({
    value: model.path,
    label: `${model.label} (${t("settings.correction.local.sizeGb", {
      size: formatGb(model.size_bytes, i18n.language),
    })})`,
  }));

  const timeoutOptions = TIMEOUT_VALUES.map((value) => ({
    value,
    label: t(`settings.advanced.modelUnload.options.${value}`),
  }));

  const fallback = settings.fallback;
  const fallbackProviderValue = fallback?.provider_id ?? NONE;

  const handleFallbackProvider = (value: string) => {
    if (value === fallbackProviderValue) return;
    onChange({
      fallback:
        value === NONE
          ? null
          : { provider_id: value, model: savedModels[value] ?? "" },
    });
  };

  return (
    <>
      <SettingsGroup title={t("settings.correction.provider.title")}>
        <SettingContainer
          title={t("settings.correction.provider.label")}
          description={t("settings.correction.provider.description")}
          descriptionMode="tooltip"
          layout="horizontal"
          grouped={true}
        >
          <Dropdown
            options={providerOptions}
            selectedValue={providerValue}
            onSelect={handleProvider}
          />
        </SettingContainer>

        {!cloud ? (
          <>
            <SettingContainer
              title={t("settings.correction.local.folder.title")}
              description={t("settings.correction.local.folder.description")}
              descriptionMode="tooltip"
              layout="stacked"
              grouped={true}
            >
              <div className="flex items-center gap-2">
                <Input
                  type="text"
                  value={dirDraft}
                  onChange={(event) => setDirDraft(event.target.value)}
                  onBlur={commitDir}
                  placeholder={t(
                    "settings.correction.local.folder.placeholder",
                  )}
                  variant="compact"
                  className="flex-1 min-w-0"
                />
                <Button
                  variant="secondary"
                  size="md"
                  onClick={pickFolder}
                  className="flex items-center gap-1 shrink-0"
                >
                  <FolderOpen className="h-4 w-4" />
                  {t("settings.correction.local.folder.choose")}
                </Button>
              </div>
            </SettingContainer>

            <SettingContainer
              title={t("settings.correction.local.model.title")}
              description={t("settings.correction.local.model.description")}
              descriptionMode="tooltip"
              layout="stacked"
              grouped={true}
            >
              {models.length === 0 ? (
                <p className="text-sm text-ink-3">
                  {t("settings.correction.local.model.empty")}
                </p>
              ) : (
                <Dropdown
                  options={modelOptions}
                  selectedValue={settings.local_model_path}
                  onSelect={(path) => onChange({ local_model_path: path })}
                  placeholder={t("settings.correction.local.model.placeholder")}
                  className="w-full"
                />
              )}
            </SettingContainer>

            <SettingContainer
              title={t("settings.correction.local.unload.title")}
              description={t("settings.correction.local.unload.description")}
              descriptionMode="tooltip"
              layout="horizontal"
              grouped={true}
            >
              <Dropdown
                options={timeoutOptions}
                selectedValue={settings.unload_timeout}
                onSelect={(value) =>
                  onChange({ unload_timeout: value as ModelUnloadTimeout })
                }
              />
            </SettingContainer>
          </>
        ) : (
          <CloudFields
            value={cloud}
            onModel={(model) =>
              onChange({
                provider: {
                  kind: "cloud",
                  provider_id: cloud.provider_id,
                  model,
                },
              })
            }
          />
        )}
      </SettingsGroup>

      <SettingsGroup title={t("settings.correction.fallback.title")}>
        <SettingContainer
          title={t("settings.correction.fallback.label")}
          description={t("settings.correction.fallback.description")}
          descriptionMode="tooltip"
          layout="horizontal"
          grouped={true}
        >
          <Dropdown
            options={[
              { value: NONE, label: t("settings.correction.fallback.none") },
              ...providers.map((provider) => ({
                value: provider.id,
                label: provider.label,
              })),
            ]}
            selectedValue={fallbackProviderValue}
            onSelect={handleFallbackProvider}
          />
        </SettingContainer>
        {fallback && (
          <CloudFields
            value={fallback}
            onModel={(model) => onChange({ fallback: { ...fallback, model } })}
          />
        )}
      </SettingsGroup>
    </>
  );
};
