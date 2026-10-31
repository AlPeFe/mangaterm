package hantho.host

import kotlinx.coroutines.InternalCoroutinesApi
import kotlinx.coroutines.MainCoroutineDispatcher
import kotlinx.coroutines.internal.MainDispatcherFactory
import java.util.concurrent.Executors
import kotlin.coroutines.CoroutineContext

/**
 * `Dispatchers.Main` para las extensiones que lo usan (en Android es el hilo de la UI): aquí es un
 * hilo propio. Se registra en META-INF/services, que es como kotlinx-coroutines lo busca.
 */
@OptIn(InternalCoroutinesApi::class)
class HostMainDispatcherFactory : MainDispatcherFactory {
    override val loadPriority = 0

    override fun createDispatcher(allFactories: List<MainDispatcherFactory>): MainCoroutineDispatcher = HostMainDispatcher
}

object HostMainDispatcher : MainCoroutineDispatcher() {
    private val executor = Executors.newSingleThreadExecutor { Thread(it, "main").apply { isDaemon = true } }

    override val immediate: MainCoroutineDispatcher get() = this

    override fun dispatch(context: CoroutineContext, block: Runnable) = executor.execute(block)

    override fun toString() = "Dispatchers.Main[hantho]"
}
