import { useEffect } from 'react'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { getRouteApi } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { toast } from 'sonner'
import { getApiErrorMessage } from '@/lib/api-error'
import { ConfigDrawer } from '@/components/config-drawer'
import { Header } from '@/components/layout/header'
import { Main } from '@/components/layout/main'
import { ProfileDropdown } from '@/components/profile-dropdown'
import { Search } from '@/components/search'
import { ThemeSwitch } from '@/components/theme-switch'
import { api } from '@/lib/axios'
import { UsersDialogs } from './components/users-dialogs'
import { UsersPrimaryButtons } from './components/users-primary-buttons'
import { UsersProvider } from './components/users-provider'
import { UsersTable } from './components/users-table'
import {
  DEFAULT_USERS_SORT,
  type UsersListParams,
  type UsersListResponse,
} from './data/schema'

const route = getRouteApi('/_authenticated/users/')

export function Users() {
  const { t } = useTranslation()
  const search = route.useSearch()
  const navigate = route.useNavigate()

  const params: UsersListParams = {
    page: search.page ?? 1,
    pageSize: search.pageSize ?? 10,
    sort: search.sort ?? DEFAULT_USERS_SORT,
    role: search.role?.length ? search.role : undefined,
    status: search.status,
    q: search.q?.trim() || undefined,
  }

  // Server-side list (spec §6.3): the URL search params are the query key,
  // so back / forward / bookmarks restore the exact view.
  const usersQuery = useQuery({
    queryKey: ['users', params],
    queryFn: async () => {
      const { data } = await api.get<UsersListResponse>('/users', { params })
      return data
    },
    placeholderData: keepPreviousData,
  })

  // The query cache only maps 500s; a list-level 422 / 403 must still reach
  // the user through the same error-code → locale mapping (spec §5.2).
  const queryError = usersQuery.error
  useEffect(() => {
    if (queryError) {
      toast.error(getApiErrorMessage(queryError, t))
    }
  }, [queryError, t])

  const total = usersQuery.data?.meta.total ?? 0

  return (
    <UsersProvider>
      <Header fixed>
        <Search />
        <div className='ms-auto flex items-center space-x-4'>
          <ThemeSwitch />
          <ConfigDrawer />
          <ProfileDropdown />
        </div>
      </Header>

      <Main className='flex flex-1 flex-col gap-4 sm:gap-6'>
        <div className='flex flex-wrap items-end justify-between gap-2'>
          <div>
            <h2 className='text-2xl font-bold tracking-tight'>
              {t('users.title')}
            </h2>
            <p className='text-muted-foreground'>{t('users.description')}</p>
            {!usersQuery.isPending && (
              <p className='text-muted-foreground text-sm'>
                {t('users.total', { total })}
              </p>
            )}
          </div>
          <UsersPrimaryButtons />
        </div>
        <UsersTable
          data={usersQuery.data?.data ?? []}
          total={total}
          search={search}
          navigate={navigate}
        />
      </Main>

      <UsersDialogs />
    </UsersProvider>
  )
}
