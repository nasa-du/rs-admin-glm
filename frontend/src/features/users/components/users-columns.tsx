import { useMemo } from 'react'
import { type ColumnDef } from '@tanstack/react-table'
import { useTranslation } from 'react-i18next'
import { formatDateTime } from '@/lib/date'
import { cn } from '@/lib/utils'
import { Badge } from '@/components/ui/badge'
import { DataTableColumnHeader } from '@/components/data-table'
import { LongText } from '@/components/long-text'
import { roleOptions, statusBadgeClasses } from '../data/data'
import { type User } from '../data/schema'

/**
 * The users table columns. Labels and dates follow the current UI language
 * (spec §6.3 / §6.5); the accessor keys are the backend sort vocabulary, so
 * a header click can write `sort=email.asc` straight into the URL.
 */
export function useUsersColumns(): ColumnDef<User>[] {
  const { t, i18n } = useTranslation()

  return useMemo(() => {
    const roleLabel = (role: User['role']) => t(`roles.${role}`)
    const statusLabel = (enabled: boolean) =>
      enabled ? t('userStatus.enabled') : t('userStatus.disabled')

    return [
      {
        accessorKey: 'displayName',
        header: ({ column }) => (
          <DataTableColumnHeader
            column={column}
            title={t('users.columns.displayName')}
          />
        ),
        cell: ({ row }) => (
          <LongText className='max-w-36 ps-3'>
            {row.getValue('displayName')}
          </LongText>
        ),
        meta: {
          title: t('users.columns.displayName'),
          className: cn(
            'drop-shadow-[0_1px_2px_rgb(0_0_0_/_0.1)] dark:drop-shadow-[0_1px_2px_rgb(255_255_255_/_0.1)]',
            'ps-0.5 max-md:sticky start-6 @4xl/content:table-cell @4xl/content:drop-shadow-none'
          ),
        },
        enableHiding: false,
      },
      {
        accessorKey: 'email',
        header: ({ column }) => (
          <DataTableColumnHeader
            column={column}
            title={t('users.columns.email')}
          />
        ),
        cell: ({ row }) => (
          <div className='w-fit ps-2 text-nowrap'>{row.getValue('email')}</div>
        ),
        meta: { title: t('users.columns.email') },
      },
      {
        accessorKey: 'role',
        header: ({ column }) => (
          <DataTableColumnHeader
            column={column}
            title={t('users.columns.role')}
          />
        ),
        cell: ({ row }) => {
          const role = row.original.role
          const roleType = roleOptions.find(({ value }) => value === role)
          return (
            <div className='flex items-center gap-x-2'>
              {roleType?.icon && (
                <roleType.icon size={16} className='text-muted-foreground' />
              )}
              <span className='text-sm'>{roleLabel(role)}</span>
            </div>
          )
        },
        meta: { title: t('users.columns.role') },
        enableHiding: false,
      },
      {
        accessorKey: 'isEnabled',
        header: ({ column }) => (
          <DataTableColumnHeader
            column={column}
            title={t('users.columns.status')}
          />
        ),
        cell: ({ row }) => {
          const enabled = row.original.isEnabled
          return (
            <Badge
              variant='outline'
              className={cn(
                statusBadgeClasses[enabled ? 'enabled' : 'disabled']
              )}
            >
              {statusLabel(enabled)}
            </Badge>
          )
        },
        meta: { title: t('users.columns.status') },
        enableHiding: false,
      },
      {
        accessorKey: 'createdAt',
        header: ({ column }) => (
          <DataTableColumnHeader
            column={column}
            title={t('users.columns.createdAt')}
          />
        ),
        cell: ({ row }) => (
          <div className='text-nowrap'>
            {formatDateTime(row.original.createdAt, i18n.language)}
          </div>
        ),
        meta: { title: t('users.columns.createdAt') },
      },
      {
        accessorKey: 'updatedAt',
        header: ({ column }) => (
          <DataTableColumnHeader
            column={column}
            title={t('users.columns.updatedAt')}
          />
        ),
        cell: ({ row }) => (
          <div className='text-nowrap'>
            {formatDateTime(row.original.updatedAt, i18n.language)}
          </div>
        ),
        meta: { title: t('users.columns.updatedAt') },
      },
    ]
  }, [t, i18n.language])
}
