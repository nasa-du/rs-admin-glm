import { useEffect, useMemo, useState } from 'react'
import {
  type OnChangeFn,
  type SortingState,
  type VisibilityState,
  flexRender,
  getCoreRowModel,
  useReactTable,
} from '@tanstack/react-table'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils'
import { type NavigateFn, useTableUrlState } from '@/hooks/use-table-url-state'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { DataTablePagination, DataTableToolbar } from '@/components/data-table'
import { roleOptions } from '../data/data'
import { DEFAULT_USERS_SORT, type User } from '../data/schema'
import { useUsersColumns } from './users-columns'

function sortingFromSearch(sort: string | undefined): SortingState {
  const [id, direction] = (sort ?? DEFAULT_USERS_SORT).split('.')
  return [{ id, desc: direction === 'desc' }]
}

function sortFromState(sorting: SortingState): string {
  const first = sorting[0]
  if (!first) return DEFAULT_USERS_SORT
  return `${first.id}.${first.desc ? 'desc' : 'asc'}`
}

type UsersTableProps = {
  data: User[]
  total: number
  search: Record<string, unknown>
  navigate: NavigateFn
}

export function UsersTable({ data, total, search, navigate }: UsersTableProps) {
  const { t } = useTranslation()
  const columns = useUsersColumns()

  // UI-only state; every server-relevant state lives in the URL (spec §6.3).
  const [columnVisibility, setColumnVisibility] = useState<VisibilityState>({})

  const sorting = useMemo(
    () => sortingFromSearch(search.sort as string | undefined),
    [search.sort]
  )

  const onSortingChange: OnChangeFn<SortingState> = (updater) => {
    const next = typeof updater === 'function' ? updater(sorting) : updater
    navigate({
      search: (prev) => ({
        ...(prev as Record<string, unknown>),
        sort: sortFromState(next),
        // A new ordering makes the old page number meaningless.
        page: undefined,
      }),
    })
  }

  const {
    globalFilter,
    onGlobalFilterChange,
    columnFilters,
    onColumnFiltersChange,
    pagination,
    onPaginationChange,
    ensurePageInRange,
  } = useTableUrlState({
    search,
    navigate,
    pagination: { defaultPage: 1, defaultPageSize: 10 },
    globalFilter: { enabled: true, key: 'q' },
    columnFilters: [
      // Status is single-valued (`status=enabled|disabled`); role is a
      // repeatable filter (`role=admin&role=user`).
      { columnId: 'isEnabled', searchKey: 'status', type: 'string' },
      { columnId: 'role', searchKey: 'role', type: 'array' },
    ],
  })

  const pageCount = Math.ceil(total / pagination.pageSize)

  // eslint-disable-next-line react-hooks/incompatible-library
  const table = useReactTable({
    data,
    columns,
    state: {
      sorting,
      pagination,
      columnFilters,
      columnVisibility,
      globalFilter,
    },
    // The server owns pagination, sorting and filtering (spec §6.3).
    manualPagination: true,
    manualSorting: true,
    manualFiltering: true,
    pageCount,
    onPaginationChange,
    onSortingChange,
    onColumnFiltersChange,
    onGlobalFilterChange,
    onColumnVisibilityChange: setColumnVisibility,
    getCoreRowModel: getCoreRowModel(),
  })

  useEffect(() => {
    if (total > 0) {
      ensurePageInRange(pageCount)
    }
  }, [total, pageCount, ensurePageInRange])

  return (
    <div
      className={cn(
        'max-sm:has-[div[role="toolbar"]]:mb-16', // Add margin bottom to the table on mobile when the toolbar is visible
        'flex flex-1 flex-col gap-4'
      )}
    >
      <DataTableToolbar
        table={table}
        searchPlaceholder={t('users.searchPlaceholder')}
        filters={[
          {
            columnId: 'isEnabled',
            title: t('users.filters.status'),
            single: true,
            options: [
              { label: t('userStatus.enabled'), value: 'enabled' },
              { label: t('userStatus.disabled'), value: 'disabled' },
            ],
          },
          {
            columnId: 'role',
            title: t('users.filters.role'),
            options: roleOptions.map(({ value, icon }) => ({
              label: t(`roles.${value}`),
              value,
              icon,
            })),
          },
        ]}
      />
      <div className='overflow-hidden rounded-md border'>
        <Table>
          <TableHeader>
            {table.getHeaderGroups().map((headerGroup) => (
              <TableRow key={headerGroup.id} className='group/row'>
                {headerGroup.headers.map((header) => {
                  return (
                    <TableHead
                      key={header.id}
                      colSpan={header.colSpan}
                      className={cn(
                        'bg-background group-hover/row:bg-muted',
                        header.column.columnDef.meta?.className,
                        header.column.columnDef.meta?.thClassName
                      )}
                    >
                      {header.isPlaceholder
                        ? null
                        : flexRender(
                            header.column.columnDef.header,
                            header.getContext()
                          )}
                    </TableHead>
                  )
                })}
              </TableRow>
            ))}
          </TableHeader>
          <TableBody>
            {table.getRowModel().rows?.length ? (
              table.getRowModel().rows.map((row) => (
                <TableRow key={row.id} className='group/row'>
                  {row.getVisibleCells().map((cell) => (
                    <TableCell
                      key={cell.id}
                      className={cn(
                        'bg-background group-hover/row:bg-muted',
                        cell.column.columnDef.meta?.className,
                        cell.column.columnDef.meta?.tdClassName
                      )}
                    >
                      {flexRender(
                        cell.column.columnDef.cell,
                        cell.getContext()
                      )}
                    </TableCell>
                  ))}
                </TableRow>
              ))
            ) : (
              <TableRow>
                <TableCell
                  colSpan={columns.length}
                  className='h-24 text-center'
                >
                  {t('users.noResults')}
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>
      <DataTablePagination table={table} className='mt-auto' />
    </div>
  )
}
