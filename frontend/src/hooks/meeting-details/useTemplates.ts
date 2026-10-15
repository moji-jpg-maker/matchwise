import { useState, useEffect, useCallback, useMemo } from 'react';
import { invoke as invokeTauri } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { useI18n, lookup } from '@/i18n';
import { en } from '@/i18n/en';
import { fa } from '@/i18n/fa';

export function useTemplates() {
  const { t, locale } = useI18n();
  const [fetchedTemplates, setFetchedTemplates] = useState<Array<{
    id: string;
    name: string;
    description: string;
  }>>([]);
  const availableTemplates = useMemo(() => {
    const catalog = locale === 'fa' ? fa : en;
    return fetchedTemplates.map((template) => ({
      ...template,
      name: lookup(catalog, `templateInfo.${template.id}.name`) ?? template.name,
      description: lookup(catalog, `templateInfo.${template.id}.description`) ?? template.description,
    }));
  }, [fetchedTemplates, locale]);
  const [selectedTemplate, setSelectedTemplate] = useState<string>('standard_meeting');

  // Fetch available templates on mount
  useEffect(() => {
    const fetchTemplates = async () => {
      try {
        const templates = await invokeTauri('api_list_templates') as Array<{
          id: string;
          name: string;
          description: string;
        }>;
        console.log('Available templates:', templates);
        setFetchedTemplates(templates);
      } catch (error) {
        console.error('Failed to fetch templates:', error);
      }
    };
    fetchTemplates();
  }, []);

  // Handle template selection
  const handleTemplateSelection = useCallback((templateId: string, templateName: string) => {
    setSelectedTemplate(templateId);
    toast.success(t('meetingUi.templateSelected'), {
      description: t('meetingUi.templateUsing', { name: templateName }),
    });
  }, [t]);

  return {
    availableTemplates,
    selectedTemplate,
    handleTemplateSelection,
  };
}
